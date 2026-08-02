/* Small, dependency-free globals available before user code. */
globalThis.global = globalThis;
globalThis.globalThis = globalThis;
const __nodeProxySet = new WeakSet();
const __nodeNativeProxy = globalThis.Proxy;
globalThis.Proxy = function (target, handlers) {
  const proxy = new __nodeNativeProxy(target, handlers);
  __nodeProxySet.add(proxy);
  return proxy;
};
const __quenchQueueMicrotask = globalThis.queueMicrotask;
globalThis.__quench_async_error = "";
globalThis.queueMicrotask = (callback) =>
  __quenchQueueMicrotask(() => {
    try {
      callback();
    } catch (error) {
      if (!globalThis.__quench_async_error)
        globalThis.__quench_async_error = String(
          error && (error.stack || error),
        );
    }
  });

globalThis.__nodeFormat = (args) =>
  args
    .map((value) => {
      try {
        return typeof value === "string" ? value : JSON.stringify(value);
      } catch (_) {
        return String(value);
      }
    })
    .join(" ");
globalThis.console = globalThis.console || {};
for (const method of ["log", "info", "warn", "error", "debug"]) {
  globalThis.console[method] = (...args) =>
    globalThis.__quench_console_write(globalThis.__nodeFormat(args));
}
globalThis.console.dir = (value) =>
  globalThis.__quench_console_write(globalThis.__nodeFormat([value]));
globalThis.console.assert = (condition, ...args) => {
  if (!condition) globalThis.console.error(...args);
};
const consoleTimers = {};
const consoleCounts = {};
globalThis.console.count = (label = "default") => {
  consoleCounts[label] = (consoleCounts[label] || 0) + 1;
  globalThis.__quench_console_write(`${label}: ${consoleCounts[label]}`);
};
globalThis.console.countReset = (label = "default") => {
  consoleCounts[label] = 0;
};
globalThis.console.clear = () => undefined;
globalThis.console.time = (label = "default") => {
  consoleTimers[label] = BigInt(globalThis.__quench_now_ns());
};
globalThis.console.timeLog = (label = "default", ...args) => {
  if (consoleTimers[label] === undefined) return;
  globalThis.__quench_console_write(
    `${label}: ${Number(BigInt(globalThis.__quench_now_ns()) - consoleTimers[label]) / 1e6} ms ${globalThis.__nodeFormat(args)}`,
  );
};
globalThis.console.timeEnd = (label = "default") => {
  if (consoleTimers[label] === undefined) return;
  globalThis.__quench_console_write(
    `${label}: ${Number(BigInt(globalThis.__quench_now_ns()) - consoleTimers[label]) / 1e6} ms`,
  );
  delete consoleTimers[label];
};

globalThis.process = {
  env: new Proxy(
    {},
    {
      get: (_, key) =>
        typeof key === "string" ? globalThis.__quench_env_get(key) : undefined,
      set: (_, key, value) => {
        globalThis.__quench_env_set(String(key), String(value));
        globalThis.__quench_env_keys = [
          ...new Set([...globalThis.__quench_env_keys, String(key)]),
        ];
        return true;
      },
      deleteProperty: (_, key) => {
        globalThis.__quench_env_delete(String(key));
        globalThis.__quench_env_keys = globalThis.__quench_env_keys.filter(
          (item) => item !== String(key),
        );
        return true;
      },
      has: (_, key) =>
        typeof key === "string" &&
        globalThis.__quench_env_get(key) !== undefined,
      ownKeys: () => globalThis.__quench_env_keys,
      getOwnPropertyDescriptor: (_, key) => ({
        enumerable: true,
        configurable: true,
        value: globalThis.__quench_env_get(String(key)),
      }),
    },
  ),
  argv: [globalThis.__quench_exec_path, ...globalThis.__quench_argv.slice(1)],
  execPath: globalThis.__quench_exec_path,
  pid: globalThis.__quench_pid,
  ppid: globalThis.__quench_ppid,
  getuid: () => globalThis.__quench_getuid,
  geteuid: () => globalThis.__quench_geteuid,
  getgid: () => globalThis.__quench_getgid,
  getegid: () => globalThis.__quench_getegid,
  platform:
    globalThis.__quench_platform === "macos"
      ? "darwin"
      : globalThis.__quench_platform,
  arch:
    globalThis.__quench_arch === "aarch64" ? "arm64" : globalThis.__quench_arch,
  version: "v20.0.0",
  versions: { node: "20.0.0", v8: "0.0.0-quench", uv: "0.0.0" },
  release: { name: "node", lts: "Quench" },
  config: {
    variables: {
      v8_enable_i18n_support: false,
      v8_enable_temporal_support: false,
      node_shared: false,
      node_use_ffi: false,
    },
  },
  features: { inspector: false, tls: false, quic: false, dtls: false },
  cwd: () => globalThis.__quench_cwd_get(),
  chdir: (value) => globalThis.__quench_chdir(String(value)),
  exitCode: 0,
  umask: (mask) =>
    globalThis.__quench_umask(mask === undefined ? undefined : Number(mask)),
  nextTick: (callback, ...args) => queueMicrotask(() => callback(...args)),
  hrtime: (previous) => {
    const ns = BigInt(globalThis.__quench_now_ns());
    const current = [Number(ns / 1000000000n), Number(ns % 1000000000n)];
    if (!previous) return current;
    let seconds = current[0] - previous[0];
    let nanos = current[1] - previous[1];
    if (nanos < 0) {
      seconds--;
      nanos += 1000000000;
    }
    return [seconds, nanos];
  },
};
process.hrtime.bigint = () => BigInt(globalThis.__quench_now_ns());

globalThis.setImmediate = (callback, ...args) =>
  queueMicrotask(() => callback(...args));
globalThis.clearImmediate = () => undefined;
globalThis.setTimeout = (callback, _delay = 0, ...args) => {
  const id = { active: true };
  queueMicrotask(() => {
    if (id.active) callback(...args);
  });
  return id;
};
globalThis.clearTimeout = (id) => {
  if (id) id.active = false;
};
globalThis.setInterval = (callback, _delay = 0, ...args) =>
  setTimeout(callback, _delay, ...args);
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
  setTimeout: (_delay = 0, value) =>
    new Promise((resolve) => queueMicrotask(() => resolve(value))),
  setImmediate: (value) =>
    new Promise((resolve) => queueMicrotask(() => resolve(value))),
};

const processListeners = {};
process.on = (event, listener) => {
  (processListeners[event] ||= []).push(listener);
  return process;
};
process.once = (event, listener) => {
  const once = (...args) => {
    process.removeListener(event, once);
    listener(...args);
  };
  return process.on(event, once);
};
process.removeListener = (event, listener) => {
  processListeners[event] = (processListeners[event] || []).filter(
    (item) => item !== listener,
  );
  return process;
};
process.removeAllListeners = (event) => {
  if (event) delete processListeners[event];
  else
    Object.keys(processListeners).forEach(
      (key) => delete processListeners[key],
    );
};
process.emit = (event, ...args) => {
  const listeners = processListeners[event] || [];
  listeners.forEach((listener) => listener(...args));
  return listeners.length > 0;
};
process.emitWarning = (warning, options = {}) => {
  const message = warning instanceof Error ? warning.message : String(warning);
  process.emit("warning", {
    name: options.name || "Warning",
    message,
    code: options.code,
  });
};

class NodeBuffer extends Uint8Array {
  get parent() {
    if (this === NodeBuffer.prototype) return undefined;
    return this.buffer;
  }
  get offset() {
    if (this === NodeBuffer.prototype) return undefined;
    return this.byteOffset;
  }
  static from(value, encoding, length) {
    if (typeof encoding === "string") encoding = encoding.toLowerCase();
    if (
      typeof value === "string" &&
      encoding !== undefined &&
      !NodeBuffer.isEncoding(encoding)
    ) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    if (value instanceof ArrayBuffer) {
      let offset = Number(encoding);
      if (!Number.isFinite(offset)) offset = Number.isNaN(offset) ? 0 : offset;
      offset = Math.trunc(offset);
      if (offset < 0 || offset > value.byteLength) {
        const error = new RangeError('"offset" is outside of buffer bounds');
        error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
        throw error;
      }
      let size =
        length === undefined
          ? value.byteLength - offset
          : Math.trunc(Number(length));
      if (!Number.isFinite(size) || Number.isNaN(size)) size = 0;
      if (size < 0 || offset + size > value.byteLength) {
        const error = new RangeError('"length" is outside of buffer bounds');
        error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
        throw error;
      }
      return new NodeBuffer(value, offset, size);
    }
    if (value instanceof String)
      return NodeBuffer.from(String(value), encoding);
    if (
      value &&
      typeof value === "object" &&
      typeof value[Symbol.toPrimitive] === "function"
    ) {
      const primitive = value[Symbol.toPrimitive]("string");
      if (typeof primitive === "string")
        return NodeBuffer.from(primitive, encoding);
    }
    if (typeof value === "string") {
      if (encoding === "hex") {
        const output = new NodeBuffer(Math.floor(value.length / 2));
        let written = 0;
        for (
          let i = 0;
          i + 1 < value.length && written < output.length;
          i += 2
        ) {
          if (!/^[0-9a-f]{2}$/i.test(value.slice(i, i + 2))) break;
          output[written++] = parseInt(value.slice(i, i + 2), 16);
        }
        return output.subarray(0, written);
      }
      if (encoding === "base64" || encoding === "base64url") {
        if (/^\s*=/.test(value)) return new NodeBuffer(0);
        const alphabet =
          "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        const clean = value
          .replace(/\s+/g, "")
          .replace(/[^A-Za-z0-9+/_-]/g, "")
          .replace(/=+$/, "")
          .replace(/-/g, "+")
          .replace(/_/g, "/");
        const output = new NodeBuffer(Math.floor((clean.length * 6) / 8));
        let buffer = 0;
        let bits = 0;
        let index = 0;
        for (const char of clean) {
          buffer = (buffer << 6) | alphabet.indexOf(char);
          bits += 6;
          if (bits >= 8) {
            bits -= 8;
            output[index++] = (buffer >> bits) & 255;
          }
        }
        return output;
      }
      if (
        encoding === "ascii" ||
        encoding === "latin1" ||
        encoding === "binary"
      ) {
        const output = new NodeBuffer(value.length);
        for (let i = 0; i < value.length; i++)
          output[i] = value.charCodeAt(i) & 0xff;
        return output;
      }
      if (
        encoding === "ucs2" ||
        encoding === "ucs-2" ||
        encoding === "utf16le" ||
        encoding === "utf-16le"
      ) {
        const output = new NodeBuffer(value.length * 2);
        for (let i = 0; i < value.length; i++) {
          const code = value.charCodeAt(i);
          output[i * 2] = code & 0xff;
          output[i * 2 + 1] = code >> 8;
        }
        return output;
      }
      let normalized = "";
      for (let i = 0; i < value.length; i++) {
        const code = value.charCodeAt(i);
        if (code >= 0xd800 && code <= 0xdbff) {
          if (i + 1 < value.length) {
            const next = value.charCodeAt(i + 1);
            if (next >= 0xdc00 && next <= 0xdfff) {
              normalized += value[i++] + value[i];
              continue;
            }
          }
          normalized += "\ufffd";
        } else if (code >= 0xdc00 && code <= 0xdfff) normalized += "\ufffd";
        else normalized += value[i];
      }
      return new NodeBuffer(new NodeTextEncoder().encode(normalized));
    }
    if (value && value.type === "Buffer" && Array.isArray(value.data))
      return new NodeBuffer(value.data);
    if (Array.isArray(value) || ArrayBuffer.isView(value))
      return new NodeBuffer(value);
    if (value && typeof value === "object" && "length" in value) {
      const length = Math.max(0, Math.trunc(Number(value.length)) || 0);
      const output = new NodeBuffer(length);
      for (let i = 0; i < length; i++) output[i] = Number(value[i]) || 0;
      return output;
    }
    const error = new TypeError(
      "The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object",
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  static alloc(size, fill = 0, encoding) {
    const length = NodeBuffer._validateSize(size);
    return new NodeBuffer(length).fill(fill, 0, length, encoding);
  }
  static allocUnsafe(size) {
    return new NodeBuffer(NodeBuffer._validateSize(size));
  }
  static allocUnsafeSlow(size) {
    return new NodeBuffer(NodeBuffer._validateSize(size));
  }
  static _validateSize(size) {
    if (typeof size !== "number") {
      const error = new TypeError('The "size" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(size) || size < 0 || size > 0x7fffffff) {
      const error = new RangeError('The value of "size" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return Math.trunc(size);
  }
  static of(...values) {
    return new NodeBuffer(values);
  }
  static copyBytesFrom(view, offset = 0, length = view.byteLength) {
    if (!ArrayBuffer.isView(view)) {
      const error = new TypeError(
        'The "view" argument must be an instance of TypedArray',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    offset = Math.trunc(Number(offset));
    length = Math.trunc(Number(length));
    if (
      !Number.isFinite(offset) ||
      !Number.isFinite(length) ||
      offset < 0 ||
      length < 0 ||
      offset + length > view.byteLength
    ) {
      const error = new RangeError(
        "The requested range is outside the bounds of the view",
      );
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const bytes = new Uint8Array(view.buffer, view.byteOffset + offset, length);
    const output = new NodeBuffer(length);
    output.set(bytes);
    return output;
  }
  static isBuffer(value) {
    return value instanceof NodeBuffer;
  }
  static isEncoding(encoding) {
    if (typeof encoding !== "string") return false;
    return [
      "hex",
      "utf8",
      "utf-8",
      "ascii",
      "latin1",
      "binary",
      "base64",
      "base64url",
      "ucs2",
      "ucs-2",
      "utf16le",
      "utf-16le",
    ].includes(encoding.toLowerCase());
  }
  static isAscii(value) {
    if (!(value instanceof Uint8Array)) {
      const error = new TypeError(
        'The "input" argument must be an instance of Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return value.every((byte) => byte < 0x80);
  }
  static isUtf8(value) {
    if (!(value instanceof Uint8Array)) {
      const error = new TypeError(
        'The "input" argument must be an instance of Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    for (let i = 0; i < value.length; i++) {
      const byte = value[i];
      let needed = 0;
      let code = 0;
      if (byte <= 0x7f) continue;
      if (byte >= 0xc2 && byte <= 0xdf) {
        needed = 1;
        code = byte & 0x1f;
      } else if (byte >= 0xe0 && byte <= 0xef) {
        needed = 2;
        code = byte & 0x0f;
      } else if (byte >= 0xf0 && byte <= 0xf4) {
        needed = 3;
        code = byte & 0x07;
      } else return false;
      for (let j = 1; j <= needed; j++) {
        if (i + j >= value.length || value[i + j] < 0x80 || value[i + j] > 0xbf)
          return false;
        code = (code << 6) | (value[i + j] & 0x3f);
      }
      if (
        (needed === 2 && code < 0x800) ||
        (needed === 3 && code < 0x10000) ||
        code > 0x10ffff ||
        (code >= 0xd800 && code <= 0xdfff)
      )
        return false;
      i += needed;
    }
    return true;
  }
  static compare(left, right) {
    if (!(left instanceof Uint8Array)) {
      const error = new TypeError(
        'The "buf1" argument must be an instance of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!(right instanceof Uint8Array)) {
      const error = new TypeError(
        'The "buf2" argument must be an instance of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const a = NodeBuffer.from(left);
    const b = NodeBuffer.from(right);
    const length = Math.min(a.length, b.length);
    for (let i = 0; i < length; i++) {
      if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
    }
    return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
  }
  static byteLength(value, encoding = "utf8") {
    if (typeof value === "string") {
      const normalized = String(encoding || "utf8").toLowerCase();
      if (
        normalized === "ascii" ||
        normalized === "latin1" ||
        normalized === "binary"
      )
        return value.length;
      if (
        normalized === "ucs2" ||
        normalized === "ucs-2" ||
        normalized === "utf16le" ||
        normalized === "utf-16le"
      )
        return value.length * 2;
      if (normalized === "hex") return Math.floor(value.length / 2);
      if (normalized === "base64" || normalized === "base64url")
        return NodeBuffer.from(value, normalized).length;
      return new NodeTextEncoder().encode(value).length;
    }
    if (value instanceof ArrayBuffer || ArrayBuffer.isView(value))
      return value.byteLength;
    const error = new TypeError(
      'The "string" argument must be of type string or an instance of Buffer or ArrayBuffer',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  subarray(begin = 0, end = this.length) {
    const index = (value, fallback) => {
      const number = Math.trunc(Number(value));
      if (Number.isNaN(number)) return fallback;
      return Math.max(
        0,
        Math.min(this.length, number < 0 ? this.length + number : number),
      );
    };
    const start = index(begin, 0);
    const finish = Math.max(start, index(end, this.length));
    const output = new NodeBuffer(finish - start);
    for (let i = start; i < finish; i++) output[i - start] = this[i];
    Object.defineProperty(output, "parent", { value: this.parent });
    return output;
  }
  slice(start = 0, end = this.length) {
    return this.subarray(start, end);
  }
  copy(target, targetStart = 0, sourceStart = 0, sourceEnd = this.length) {
    if (!(this instanceof Uint8Array)) {
      const error = new TypeError(
        "Method Buffer.prototype.copy called on incompatible receiver",
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!ArrayBuffer.isView(target)) {
      const error = new TypeError(
        'The "target" argument must be an instance of Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const number = (value) => {
      const result = Math.trunc(Number(value));
      return Number.isNaN(result) ? 0 : result;
    };
    targetStart = number(targetStart);
    sourceStart = number(sourceStart);
    sourceEnd = number(sourceEnd);
    if (targetStart < 0 || sourceStart < 0 || sourceEnd < 0) {
      const error = new RangeError("The value is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (sourceStart > this.length) {
      const error = new RangeError("The value of sourceStart is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const targetBytes =
      target instanceof Uint8Array
        ? target
        : new Uint8Array(target.buffer, target.byteOffset, target.byteLength);
    if (targetStart >= targetBytes.length || sourceStart >= sourceEnd) return 0;
    const end = Math.min(sourceEnd, this.length);
    const count = Math.min(end - sourceStart, targetBytes.length - targetStart);
    const bytes = new Uint8Array(count);
    bytes.set(this.subarray(sourceStart, sourceStart + count));
    targetBytes.set(bytes, targetStart);
    return count;
  }
  static concat(list, totalLength) {
    if (!Array.isArray(list)) {
      const error = new TypeError(
        'The "list" argument must be an instance of Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (list.some((item) => !ArrayBuffer.isView(item))) {
      const error = new TypeError(
        'The "list" argument must contain only typed arrays',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      totalLength !== undefined &&
      (typeof totalLength !== "number" ||
        !Number.isInteger(totalLength) ||
        totalLength < 0)
    ) {
      const error = new RangeError('The value of "length" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const length =
      totalLength === undefined
        ? list.reduce((sum, item) => sum + item.byteLength, 0)
        : totalLength;
    const output = new NodeBuffer(length);
    let offset = 0;
    list.forEach((item) => {
      const bytes = new Uint8Array(
        item.buffer,
        item.byteOffset,
        item.byteLength,
      );
      const count = Math.min(bytes.length, length - offset);
      if (count > 0) output.set(bytes.subarray(0, count), offset);
      offset += count;
    });
    return output;
  }
  fill(value = 0, start = 0, end = this.length, encoding = "utf8") {
    if (typeof start === "string") {
      encoding = start;
      start = 0;
      end = this.length;
    } else if (typeof end === "string") {
      encoding = end;
      end = this.length;
    }
    if (typeof start !== "number" || typeof end !== "number") {
      const error = new TypeError(
        'The "start" and "end" arguments must be of type number',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const toIndex = (input, fallback) => {
      const number = Math.trunc(Number(input));
      return Number.isNaN(number) ? fallback : number;
    };
    start = toIndex(start, 0);
    end = toIndex(end, this.length);
    if (start < 0 || end < 0 || start > this.length || end > this.length) {
      const error = new RangeError("The value is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    let pattern;
    if (value === null || value === undefined || typeof value === "number")
      pattern = new NodeBuffer([Number(value) || 0]);
    else if (typeof value === "string") {
      if (
        String(encoding).toLowerCase() === "hex" &&
        (value.length % 2 || !/^[0-9a-f]*$/i.test(value))
      ) {
        const error = new TypeError('The "value" argument is invalid');
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      pattern = NodeBuffer.from(value, encoding);
    } else if (ArrayBuffer.isView(value))
      pattern = new Uint8Array(
        value.buffer,
        value.byteOffset,
        value.byteLength,
      );
    else pattern = NodeBuffer.from(String(value));
    if (pattern.length === 0) return this;
    for (let i = start; i < end; i++)
      this[i] = pattern[(i - start) % pattern.length];
    return this;
  }
  toString(encoding = "utf8", start = 0, end = this.length) {
    if (start !== 0 || end !== this.length) {
      const normalize = (value, fallback) => {
        const number = Math.trunc(Number(value));
        return Number.isNaN(number)
          ? fallback
          : Math.max(
              0,
              Math.min(this.length, number < 0 ? this.length + number : number),
            );
      };
      const first = normalize(start, 0);
      const last = normalize(end, this.length);
      if (last <= first) return "";
      return this.subarray(first, Math.max(first, last)).toString(encoding);
    }
    encoding = String(encoding).toLowerCase();
    if (!NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    if (encoding === "hex")
      return Array.from(this, (byte) =>
        byte.toString(16).padStart(2, "0"),
      ).join("");
    if (encoding === "base64" || encoding === "base64url") {
      const alphabet =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
      let result = "";
      for (let i = 0; i < this.length; i += 3) {
        const n =
          (this[i] << 16) | ((this[i + 1] || 0) << 8) | (this[i + 2] || 0);
        result +=
          alphabet[(n >> 18) & 63] +
          alphabet[(n >> 12) & 63] +
          (i + 1 < this.length ? alphabet[(n >> 6) & 63] : "=") +
          (i + 2 < this.length ? alphabet[n & 63] : "=");
      }
      return encoding === "base64url"
        ? result.replace(/=+$/, "").replace(/\+/g, "-").replace(/\//g, "_")
        : result;
    }
    if (encoding === "latin1" || encoding === "binary")
      return Array.from(this, (byte) => String.fromCharCode(byte)).join("");
    if (encoding === "ascii")
      return Array.from(this, (byte) => String.fromCharCode(byte & 0x7f)).join(
        "",
      );
    if (
      encoding === "utf16le" ||
      encoding === "utf-16le" ||
      encoding === "ucs2" ||
      encoding === "ucs-2"
    ) {
      let result = "";
      for (let i = 0; i + 1 < this.length; i += 2)
        result += String.fromCharCode(this[i] | (this[i + 1] << 8));
      return result;
    }
    return new NodeTextDecoder().decode(this);
  }
  equals(other) {
    if (!(other instanceof Uint8Array)) {
      const error = new TypeError(
        'The "otherBuffer" argument must be an instance of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return (
      this.length === other.length &&
      this.every((value, index) => value === other[index])
    );
  }
  compare(target) {
    if (!(target instanceof Uint8Array)) {
      const error = new TypeError(
        'The "target" argument must be an instance of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return NodeBuffer.compare(this, target);
  }
  _swap(width) {
    if (this.length % width !== 0)
      throw new Error(`Buffer size must be a multiple of ${width * 8}-bits`);
    for (let offset = 0; offset < this.length; offset += width) {
      for (let i = 0; i < width / 2; i++) {
        const value = this[offset + i];
        this[offset + i] = this[offset + width - i - 1];
        this[offset + width - i - 1] = value;
      }
    }
    return this;
  }
  swap16() {
    return this._swap(2);
  }
  swap32() {
    return this._swap(4);
  }
  swap64() {
    return this._swap(8);
  }
  _readBigInt(offset, littleEndian, signed) {
    this._integerOffset(offset, 8);
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    return signed
      ? view.getBigInt64(offset, littleEndian)
      : view.getBigUint64(offset, littleEndian);
  }
  _writeBigInt(value, offset, littleEndian, signed) {
    this._integerOffset(offset, 8);
    if (typeof value !== "bigint")
      throw new TypeError('The "value" argument must be a bigint');
    const min = signed ? -(1n << 63n) : 0n;
    const max = signed ? (1n << 63n) - 1n : (1n << 64n) - 1n;
    if (value < min || value > max) {
      const error = new RangeError('The value of "value" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (signed) view.setBigInt64(offset, value, littleEndian);
    else view.setBigUint64(offset, value, littleEndian);
    return offset + 8;
  }
  readBigInt64LE(offset = 0) {
    return this._readBigInt(offset, true, true);
  }
  readBigInt64BE(offset = 0) {
    return this._readBigInt(offset, false, true);
  }
  readBigUInt64LE(offset = 0) {
    return this._readBigInt(offset, true, false);
  }
  readBigUInt64BE(offset = 0) {
    return this._readBigInt(offset, false, false);
  }
  writeBigInt64LE(value, offset = 0) {
    return this._writeBigInt(value, offset, true, true);
  }
  writeBigInt64BE(value, offset = 0) {
    return this._writeBigInt(value, offset, false, true);
  }
  writeBigUInt64LE(value, offset = 0) {
    return this._writeBigInt(value, offset, true, false);
  }
  writeBigUInt64BE(value, offset = 0) {
    return this._writeBigInt(value, offset, false, false);
  }
  toJSON() {
    return { type: "Buffer", data: Array.from(this) };
  }
  inspect() {
    const limit = Math.min(this.length, NodeBuffer.INSPECT_MAX_BYTES);
    const bytes = Array.from(this.subarray(0, limit), (byte) =>
      byte.toString(16).padStart(2, "0"),
    );
    return `<Buffer ${bytes.join(" ")}${limit < this.length ? ` ... ${this.length - limit} more byte${this.length - limit === 1 ? "" : "s"}` : ""}>`;
  }
  toLocaleString(...args) {
    return this.toString(...args);
  }
  includes(value, byteOffset = 0, encoding) {
    if (
      typeof value !== "number" &&
      typeof value !== "string" &&
      !(value instanceof Uint8Array)
    ) {
      const error = new TypeError(
        'The "value" argument must be one of type number or string or an instance of Buffer or Uint8Array.',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let start = Number(byteOffset);
    if (Number.isNaN(start) || start === -Infinity) start = 0;
    if (start === Infinity)
      return (
        value === "" || (value instanceof Uint8Array && value.length === 0)
      );
    start =
      start < 0
        ? Math.max(this.length + Math.trunc(start), 0)
        : Math.trunc(start);
    if (
      (encoding === "ucs2" || encoding === "ucs-2" || encoding === "utf16le") &&
      start % 2 !== 0
    )
      return false;
    const needle =
      typeof value === "number"
        ? NodeBuffer.from([value & 0xff])
        : NodeBuffer.from(value, encoding);
    if (needle.length === 0) return true;
    for (let i = start; i + needle.length <= this.length; i++) {
      let match = true;
      for (let j = 0; j < needle.length; j++)
        if (this[i + j] !== needle[j]) {
          match = false;
          break;
        }
      if (match) return true;
    }
    return false;
  }
  indexOf(value, byteOffset = 0, encoding) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = 0;
    }
    const needle =
      typeof value === "number"
        ? new Uint8Array([value & 0xff])
        : typeof value === "string"
          ? NodeBuffer.from(value, encoding)
          : value;
    const offset = Number(byteOffset);
    let start =
      Number.isNaN(offset) || offset === -Infinity ? 0 : Math.trunc(offset);
    if (start < 0) start = Math.max(this.length + start, 0);
    if (
      (encoding === "ucs2" ||
        encoding === "ucs-2" ||
        encoding === "utf16le" ||
        encoding === "utf-16le") &&
      start % 2 !== 0
    )
      return -1;
    if (start > this.length || start === Infinity)
      return needle.length === 0 ? this.length : -1;
    if (needle.length === 0) return start;
    for (let i = start; i + needle.length <= this.length; i++) {
      let match = true;
      for (let j = 0; j < needle.length; j++)
        if (this[i + j] !== needle[j]) match = false;
      if (match) return i;
    }
    return -1;
  }
  lastIndexOf(value, byteOffset = this.length - 1, encoding) {
    if (typeof byteOffset === "string") {
      encoding = byteOffset;
      byteOffset = this.length - 1;
    }
    const needle =
      typeof value === "number"
        ? new Uint8Array([value & 0xff])
        : typeof value === "string"
          ? NodeBuffer.from(value, encoding)
          : value;
    let end = Number(byteOffset);
    end =
      Number.isNaN(end) || end === Infinity ? this.length - 1 : Math.trunc(end);
    if (end < 0) end = this.length + end;
    if (needle.length === 0) return Math.max(0, Math.min(end, this.length));
    for (let i = Math.min(end, this.length - needle.length); i >= 0; i--) {
      let match = true;
      for (let j = 0; j < needle.length; j++)
        if (this[i + j] !== needle[j]) match = false;
      if (match) return i;
    }
    return -1;
  }
  write(value, offset = 0, length, encoding = "utf8") {
    if (typeof offset === "string") {
      if (length !== undefined) {
        const error = new TypeError(
          'The "offset" argument must be of type number',
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      encoding = offset;
      offset = 0;
      length = undefined;
    }
    if (typeof length === "string") {
      encoding = length;
      length = undefined;
    }
    if (typeof encoding !== "string" || !NodeBuffer.isEncoding(encoding)) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    if (
      typeof offset !== "number" ||
      !Number.isInteger(offset) ||
      offset < 0 ||
      offset > this.length
    ) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const bytes = NodeBuffer.from(String(value), encoding);
    const requested =
      length === undefined
        ? this.length - offset
        : Math.max(0, Math.trunc(Number(length)) || 0);
    let count = Math.min(requested, this.length - offset, bytes.length);
    const normalized = String(encoding).toLowerCase();
    if (normalized === "utf8" || normalized === "utf-8") {
      let complete = 0;
      for (let i = 1; i <= String(value).length; i++) {
        const size = NodeBuffer.from(String(value).slice(0, i), "utf8").length;
        if (size > count) break;
        complete = size;
      }
      count = complete;
    }
    if (
      (normalized === "ucs2" ||
        normalized === "ucs-2" ||
        normalized === "utf16le" ||
        normalized === "utf-16le") &&
      count % 2
    )
      count--;
    this.set(bytes.subarray(0, count), offset);
    return count;
  }
  writeDoubleLE(value, offset = 0) {
    return this._writeDouble(value, offset, true);
  }
  writeDoubleBE(value, offset = 0) {
    return this._writeDouble(value, offset, false);
  }
  _writeDouble(value, offset, littleEndian) {
    if (typeof offset !== "number") {
      const error = new TypeError(
        'The "offset" argument must be of type number',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(offset)) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(offset)) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset < 0 || offset + 8 > this.length) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds",
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    new DataView(this.buffer, this.byteOffset, this.byteLength).setFloat64(
      offset,
      Number(value),
      littleEndian,
    );
    return offset + 8;
  }
  readDoubleLE(offset = 0) {
    return this._readDouble(offset, true);
  }
  readDoubleBE(offset = 0) {
    return this._readDouble(offset, false);
  }
  _readDouble(offset, littleEndian) {
    if (typeof offset !== "number") {
      const error = new TypeError(
        'The "offset" argument must be of type number',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(offset)) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset < 0 || offset + 8 > this.length) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds",
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength,
    ).getFloat64(offset, littleEndian);
  }
  _integerOffset(offset, size) {
    if (typeof offset !== "number") {
      const error = new TypeError(
        'The "offset" argument must be of type number',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(offset) || offset < 0) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (offset + size > this.length) {
      const error = new RangeError(
        "Attempt to access memory outside buffer bounds",
      );
      error.code = "ERR_BUFFER_OUT_OF_BOUNDS";
      throw error;
    }
    return offset;
  }
  _writeInteger(value, offset, size, littleEndian, signed) {
    this._integerOffset(offset, size);
    const max = signed ? 2 ** (size * 8 - 1) - 1 : 2 ** (size * 8) - 1;
    const min = signed ? -(2 ** (size * 8 - 1)) : 0;
    if (
      typeof value !== "number" ||
      !Number.isInteger(value) ||
      value < min ||
      value > max
    ) {
      const error = new RangeError('The value of "value" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (size === 1) view.setInt8(offset, value);
    else if (size === 2)
      signed
        ? view.setInt16(offset, value, littleEndian)
        : view.setUint16(offset, value, littleEndian);
    else
      signed
        ? view.setInt32(offset, value, littleEndian)
        : view.setUint32(offset, value, littleEndian);
    return offset + size;
  }
  _readInteger(offset, size, littleEndian, signed) {
    this._integerOffset(offset, size);
    const view = new DataView(this.buffer, this.byteOffset, this.byteLength);
    if (size === 1)
      return signed ? view.getInt8(offset) : view.getUint8(offset);
    if (size === 2)
      return signed
        ? view.getInt16(offset, littleEndian)
        : view.getUint16(offset, littleEndian);
    return signed
      ? view.getInt32(offset, littleEndian)
      : view.getUint32(offset, littleEndian);
  }
  readUInt8(offset = 0) {
    return this._readInteger(offset, 1, false, false);
  }
  readUInt16LE(offset = 0) {
    return this._readInteger(offset, 2, true, false);
  }
  readUInt16BE(offset = 0) {
    return this._readInteger(offset, 2, false, false);
  }
  readUInt32LE(offset = 0) {
    return this._readInteger(offset, 4, true, false);
  }
  readUInt32BE(offset = 0) {
    return this._readInteger(offset, 4, false, false);
  }
  writeUInt8(value, offset = 0) {
    return this._writeInteger(value, offset, 1, false, false);
  }
  writeUInt16LE(value, offset = 0) {
    return this._writeInteger(value, offset, 2, true, false);
  }
  writeUInt16BE(value, offset = 0) {
    return this._writeInteger(value, offset, 2, false, false);
  }
  writeUInt32LE(value, offset = 0) {
    return this._writeInteger(value, offset, 4, true, false);
  }
  writeUInt32BE(value, offset = 0) {
    return this._writeInteger(value, offset, 4, false, false);
  }
  readInt8(offset = 0) {
    return this._readInteger(offset, 1, false, true);
  }
  readInt16LE(offset = 0) {
    return this._readInteger(offset, 2, true, true);
  }
  readInt16BE(offset = 0) {
    return this._readInteger(offset, 2, false, true);
  }
  readInt32LE(offset = 0) {
    return this._readInteger(offset, 4, true, true);
  }
  readInt32BE(offset = 0) {
    return this._readInteger(offset, 4, false, true);
  }
  writeInt8(value, offset = 0) {
    return this._writeInteger(value, offset, 1, false, true);
  }
  writeInt16LE(value, offset = 0) {
    return this._writeInteger(value, offset, 2, true, true);
  }
  writeInt16BE(value, offset = 0) {
    return this._writeInteger(value, offset, 2, false, true);
  }
  writeInt32LE(value, offset = 0) {
    return this._writeInteger(value, offset, 4, true, true);
  }
  writeInt32BE(value, offset = 0) {
    return this._writeInteger(value, offset, 4, false, true);
  }
  readFloatLE(offset = 0) {
    this._integerOffset(offset, 4);
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength,
    ).getFloat32(offset, true);
  }
  readFloatBE(offset = 0) {
    this._integerOffset(offset, 4);
    return new DataView(
      this.buffer,
      this.byteOffset,
      this.byteLength,
    ).getFloat32(offset, false);
  }
  writeFloatLE(value, offset = 0) {
    this._integerOffset(offset, 4);
    new DataView(this.buffer, this.byteOffset, this.byteLength).setFloat32(
      offset,
      value,
      true,
    );
    return offset + 4;
  }
  writeFloatBE(value, offset = 0) {
    this._integerOffset(offset, 4);
    new DataView(this.buffer, this.byteOffset, this.byteLength).setFloat32(
      offset,
      value,
      false,
    );
    return offset + 4;
  }
  readUIntLE(offset, byteLength) {
    this._validateVariableInteger(0, offset, byteLength);
    let value = 0;
    for (let i = 0; i < byteLength; i++)
      value += this[offset + i] * 2 ** (8 * i);
    return value;
  }
  readUIntBE(offset, byteLength) {
    this._validateVariableInteger(0, offset, byteLength);
    let value = 0;
    for (let i = 0; i < byteLength; i++) value = value * 256 + this[offset + i];
    return value;
  }
  writeUIntLE(value, offset, byteLength) {
    this._validateVariableInteger(value, offset, byteLength, false);
    for (let i = 0; i < byteLength; i++) {
      this[offset + i] = value & 0xff;
      value = Math.floor(value / 256);
    }
    return offset + byteLength;
  }
  writeUIntBE(value, offset, byteLength) {
    this._validateVariableInteger(value, offset, byteLength, false);
    for (let i = byteLength - 1; i >= 0; i--) {
      this[offset + i] = value & 0xff;
      value = Math.floor(value / 256);
    }
    return offset + byteLength;
  }
  readIntLE(offset, byteLength) {
    const value = this.readUIntLE(offset, byteLength);
    const limit = 2 ** (byteLength * 8 - 1);
    return value >= limit ? value - 2 ** (byteLength * 8) : value;
  }
  readIntBE(offset, byteLength) {
    const value = this.readUIntBE(offset, byteLength);
    const limit = 2 ** (byteLength * 8 - 1);
    return value >= limit ? value - 2 ** (byteLength * 8) : value;
  }
  writeIntLE(value, offset, byteLength) {
    this._validateVariableInteger(value, offset, byteLength, true);
    const modulus = 2 ** (byteLength * 8);
    return this.writeUIntLE(
      value < 0 ? modulus + value : value,
      offset,
      byteLength,
    );
  }
  writeIntBE(value, offset, byteLength) {
    this._validateVariableInteger(value, offset, byteLength, true);
    const modulus = 2 ** (byteLength * 8);
    return this.writeUIntBE(
      value < 0 ? modulus + value : value,
      offset,
      byteLength,
    );
  }
  _validateVariableInteger(value, offset, byteLength, signed = false) {
    if (typeof byteLength !== "number") {
      const error = new TypeError(
        'The "byteLength" argument must be of type number',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(byteLength) || byteLength < 1 || byteLength > 6) {
      const error = new RangeError('The value of "byteLength" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    this._integerOffset(offset, byteLength);
    const min = signed ? -(2 ** (8 * byteLength - 1)) : 0;
    const max = signed
      ? 2 ** (8 * byteLength - 1) - 1
      : 2 ** (8 * byteLength) - 1;
    if (
      typeof value !== "number" ||
      !Number.isSafeInteger(value) ||
      value < min ||
      value > max
    ) {
      const error = new RangeError('The value of "value" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  }
}
globalThis.Buffer = new Proxy(NodeBuffer, {
  apply(_target, _thisArg, args) {
    if (typeof args[0] === "number") return new NodeBuffer(args[0]);
    return NodeBuffer.from(...args);
  },
});
NodeBuffer.poolSize = 8192;
NodeBuffer.prototype[Symbol.for("nodejs.util.inspect.custom")] =
  NodeBuffer.prototype.inspect;
for (const name of ["8", "16LE", "16BE", "32LE", "32BE"]) {
  NodeBuffer.prototype[`readUint${name}`] =
    NodeBuffer.prototype[`readUInt${name}`];
  NodeBuffer.prototype[`writeUint${name}`] =
    NodeBuffer.prototype[`writeUInt${name}`];
}
NodeBuffer.prototype.readUintLE = NodeBuffer.prototype.readUIntLE;
NodeBuffer.prototype.readUintBE = NodeBuffer.prototype.readUIntBE;
NodeBuffer.prototype.writeUintLE = NodeBuffer.prototype.writeUIntLE;
NodeBuffer.prototype.writeUintBE = NodeBuffer.prototype.writeUIntBE;
NodeBuffer.prototype.readBigUint64LE = NodeBuffer.prototype.readBigUInt64LE;
NodeBuffer.prototype.readBigUint64BE = NodeBuffer.prototype.readBigUInt64BE;
NodeBuffer.prototype.writeBigUint64LE = NodeBuffer.prototype.writeBigUInt64LE;
NodeBuffer.prototype.writeBigUint64BE = NodeBuffer.prototype.writeBigUInt64BE;
const nodeAtob = (value) => NodeBuffer.from(String(value), "base64").toString();
const nodeBtoa = (value) => NodeBuffer.from(String(value)).toString("base64");
class NodeTextEncoder {
  encode(value) {
    const output = [];
    for (const character of String(value)) {
      const code = character.codePointAt(0);
      if (code < 0x80) output.push(code);
      else if (code < 0x800)
        output.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
      else if (code < 0x10000)
        output.push(
          0xe0 | (code >> 12),
          0x80 | ((code >> 6) & 0x3f),
          0x80 | (code & 0x3f),
        );
      else
        output.push(
          0xf0 | (code >> 18),
          0x80 | ((code >> 12) & 0x3f),
          0x80 | ((code >> 6) & 0x3f),
          0x80 | (code & 0x3f),
        );
    }
    return new Uint8Array(output);
  }
}
globalThis.TextEncoder = NodeTextEncoder;
class NodeTextDecoder {
  decode(bytes) {
    let result = "";
    for (let i = 0; i < bytes.length; ) {
      const first = bytes[i++];
      if (first < 0x80) result += String.fromCodePoint(first);
      else if (first < 0xe0)
        result += String.fromCodePoint(
          ((first & 0x1f) << 6) | (bytes[i++] & 0x3f),
        );
      else if (first < 0xf0)
        result += String.fromCodePoint(
          ((first & 0x0f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f),
        );
      else
        result += String.fromCodePoint(
          ((first & 7) << 18) |
            ((bytes[i++] & 0x3f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f),
        );
    }
    return result;
  }
}
globalThis.TextDecoder = NodeTextDecoder;
const nodePathValue = (value) =>
  value instanceof NodeBuffer
    ? value.toString()
    : value instanceof Uint8Array
      ? new NodeTextDecoder().decode(value)
      : value instanceof globalThis.__nodeURL
        ? globalThis.__nodeUrlModule.fileURLToPath(value)
        : String(value);
const nodeFsPath = (value) => {
  if (
    typeof value === "string" ||
    value instanceof NodeBuffer ||
    value instanceof Uint8Array ||
    value instanceof globalThis.__nodeURL
  )
    return nodePathValue(value);
  const error = new TypeError(
    'The "path" argument must be of type string or an instance of Buffer or URL',
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};

globalThis.__nodeAssert = (value, message) => {
  if (!value) throw new Error(message || "Assertion failed");
};
globalThis.__nodeAssert.strictEqual = (actual, expected, message) => {
  if (!Object.is(actual, expected))
    throw new Error(message || `${actual} !== ${expected}`);
};
globalThis.__nodeAssert.equal = (actual, expected, message) => {
  if (actual != expected)
    throw new Error(message || `${actual} != ${expected}`);
};
globalThis.__nodeAssert.notStrictEqual = (actual, expected, message) => {
  if (Object.is(actual, expected))
    throw new Error(message || `${actual} === ${expected}`);
};
globalThis.__nodeAssert.notEqual = (actual, expected, message) => {
  if (actual == expected)
    throw new Error(message || `${actual} == ${expected}`);
};
globalThis.__nodeAssert.ok = globalThis.__nodeAssert;
globalThis.__nodeAssert.deepStrictEqual = (actual, expected, message) => {
  if (JSON.stringify(actual) !== JSON.stringify(expected))
    throw new Error(message || "values differ");
};
globalThis.__nodeAssert.throws = (fn, expected) => {
  let thrown = false;
  try {
    fn();
  } catch (error) {
    thrown = true;
    if (expected && expected.name && error.name !== expected.name) throw error;
  }
  if (!thrown) throw new Error("Missing expected exception");
};
globalThis.__nodeAssert.ifError = (error) => {
  if (error) throw error;
};
globalThis.__nodeAssert.doesNotThrow = (fn, message) => {
  try {
    fn();
  } catch (error) {
    throw new Error(message || `Unexpected exception: ${error}`);
  }
};
globalThis.__nodeAssert.rejects = (promiseOrFn, expected) =>
  Promise.resolve()
    .then(() =>
      typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn,
    )
    .then(
      () => {
        throw new Error("Missing expected rejection");
      },
      (error) => {
        if (expected && expected.name && error.name !== expected.name)
          throw error;
        return error;
      },
    );
globalThis.__nodeAssert.doesNotReject = (promiseOrFn, message) =>
  Promise.resolve()
    .then(() =>
      typeof promiseOrFn === "function" ? promiseOrFn() : promiseOrFn,
    )
    .catch((error) => {
      throw new Error(message || `Unexpected rejection: ${error}`);
    });
globalThis.__nodeAssert.match = (value, expression) => {
  if (!expression.test(String(value)))
    throw new Error("Value did not match expression");
};
globalThis.__nodeAssert.strict = globalThis.__nodeAssert;

globalThis.__nodePath = {
  sep: "/",
  isAbsolute: (value) => String(value).startsWith("/"),
  normalize: (value) => {
    const absolute = String(value).startsWith("/");
    const parts = String(value)
      .split("/")
      .filter((part) => part && part !== ".");
    const output = [];
    parts.forEach((part) => {
      if (part === ".." && output.length && output[output.length - 1] !== "..")
        output.pop();
      else if (part !== "..") output.push(part);
    });
    const result = (absolute ? "/" : "") + output.join("/");
    return result || (absolute ? "/" : ".");
  },
  basename: (value) => String(value).replace(/\\/g, "/").split("/").pop(),
  dirname: (value) => {
    const parts = String(value).replace(/\\/g, "/").split("/");
    parts.pop();
    return parts.join("/") || ".";
  },
  extname: (value) => {
    const name = globalThis.__nodePath.basename(value);
    const i = name.lastIndexOf(".");
    return i > 0 ? name.slice(i) : "";
  },
  join: (...parts) => globalThis.__nodePath.normalize(parts.join("/")),
  resolve: (...parts) =>
    globalThis.__nodePath.normalize(parts.filter(Boolean).join("/")),
  relative: (from, to) => {
    const a = globalThis.__nodePath.normalize(from).split("/").filter(Boolean);
    const b = globalThis.__nodePath.normalize(to).split("/").filter(Boolean);
    while (a.length && a[0] === b[0]) {
      a.shift();
      b.shift();
    }
    return [...a.map(() => ".."), ...b].join("/") || "";
  },
  parse: (value) => {
    if (typeof value !== "string") {
      const error = new TypeError('The "path" argument must be of type string');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const input = String(value);
    const base = globalThis.__nodePath.basename(input);
    const dir = globalThis.__nodePath.dirname(input);
    const ext = globalThis.__nodePath.extname(base);
    return {
      root: input.startsWith("/") ? "/" : "",
      dir,
      base,
      ext,
      name: ext ? base.slice(0, -ext.length) : base,
    };
  },
  format: (parts) => {
    if (!parts || typeof parts !== "object" || Array.isArray(parts)) {
      const error = new TypeError(
        'The "pathObject" argument must be of type object',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const dir = parts.dir || parts.root || "";
    const extension = parts.ext
      ? String(parts.ext).startsWith(".")
        ? String(parts.ext)
        : `.${parts.ext}`
      : "";
    const base = parts.base || `${parts.name || ""}${extension}`;
    if (!dir) return base;
    if (dir === "/") return `/${base}`;
    return `${dir}/${base}`;
  },
};
globalThis.__nodePath.posix = globalThis.__nodePath;
const __nodeWinPath = {
  sep: "\\",
  parse(value) {
    if (typeof value !== "string") {
      const error = new TypeError('The "path" argument must be of type string');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const input = value.replace(/\//g, "\\");
    let root = "";
    if (input.startsWith("\\\\")) {
      const parts = input.split("\\");
      if (parts.length >= 4 && parts[2] && parts[3])
        root = `\\\\${parts[2]}\\${parts[3]}\\`;
      else root = "\\";
    } else if (/^[A-Za-z]:\\/.test(input)) root = input.slice(0, 3);
    else if (/^[A-Za-z]:/.test(input)) root = input.slice(0, 2);
    else if (input.startsWith("\\")) root = "\\";
    const hadTrailingSeparator = input.length > 0 && /[\\]$/.test(input);
    const trimmed = input.replace(/[\\]+$/, "") || root;
    if (/^[A-Za-z]:$/.test(input))
      return { root: input, dir: "", base: "", ext: "", name: "" };
    if (/^[A-Za-z]:[^\\]/.test(input)) {
      const relative = input.slice(2);
      const dot = relative.lastIndexOf(".");
      const ext = dot > 0 ? relative.slice(dot) : "";
      return {
        root: input.slice(0, 2),
        dir: "",
        base: relative,
        ext,
        name: ext ? relative.slice(0, -ext.length) : relative,
      };
    }
    if (root.startsWith("\\\\") && trimmed === root.slice(0, -1))
      return { root, dir: root, base: "", ext: "", name: "" };
    const index = trimmed.lastIndexOf("\\");
    const dir =
      index >= 0
        ? (hadTrailingSeparator || (root.length === 3 && index === 2)
            ? trimmed.slice(0, index + 1)
            : trimmed.slice(0, index)) || root
        : "";
    const base = index >= 0 ? trimmed.slice(index + 1) : trimmed;
    const dot = base.lastIndexOf(".");
    const ext = dot > 0 ? base.slice(dot) : "";
    return {
      root,
      dir,
      base,
      ext,
      name: ext ? base.slice(0, -ext.length) : base,
    };
  },
  format(parts) {
    if (!parts || typeof parts !== "object" || Array.isArray(parts)) {
      const error = new TypeError(
        'The "pathObject" argument must be of type object',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const extension = parts.ext
      ? String(parts.ext).startsWith(".")
        ? String(parts.ext)
        : `.${parts.ext}`
      : "";
    const base = parts.base || `${parts.name || ""}${extension}`;
    const dir = parts.dir || parts.root || "";
    if (!dir) return base;
    if (/^[A-Za-z]:$/.test(dir)) return `${dir}${base}`;
    if (dir.endsWith("\\")) return `${dir}${base}`;
    return `${dir}\\${base}`;
  },
  basename: (value) =>
    String(value)
      .replace(/[\\/]+$/, "")
      .split(/[\\/]/)
      .pop() || "",
  dirname: (value) => {
    const input = String(value).replace(/[\\/]+$/, "");
    const index = input.lastIndexOf("\\");
    return index < 0 ? "" : input.slice(0, index) || "\\";
  },
  extname(value) {
    const base = this.basename(value);
    const index = base.lastIndexOf(".");
    return index > 0 ? base.slice(index) : "";
  },
};
__nodeWinPath.posix = globalThis.__nodePath;
__nodeWinPath.win32 = __nodeWinPath;
globalThis.__nodePath.win32 = __nodeWinPath;

globalThis.__nodeCommon = {
  mustCall: (fn, exact = 1) => {
    let calls = 0;
    const wrapped = function (...args) {
      calls++;
      wrapped.calls = calls;
      return fn(...args);
    };
    wrapped.calls = 0;
    wrapped.expected = exact;
    wrapped.__quench_index = (globalThis.__nodeCallChecks ||= []).length;
    globalThis.__nodeCallChecks.push(wrapped);
    return wrapped;
  },
  mustSucceed: (fn = () => {}) =>
    globalThis.__nodeCommon.mustCall((error, ...args) => {
      if (error) throw error;
      return fn(...args);
    }),
  mustNotCall:
    (message = "Unexpected call") =>
    () => {
      throw new Error(message);
    },
  noop: () => {},
  printSkipMessage: (message) => console.log(`# SKIP: ${message}`),
  expectsError: (_expected) => (error) => {
    if (!error) throw new Error("Expected filesystem error");
  },
  invalidArgTypeHelper: (input) =>
    input == null
      ? ` Received ${input}`
      : ` Received type ${typeof input} (${String(input)})`,
  expectWarning: (_type, _message) => {},
  mustNotMutateObjectDeep: (value) => value,
  isLinux: process.platform === "linux",
  isMacOS: process.platform === "darwin",
  isWindows: process.platform === "win32",
  isAIX: false,
  isFreeBSD: false,
  canCreateSymLink: () => process.platform !== "win32",
  getArrayBufferViews: (buffer) => [
    buffer,
    new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength),
    new DataView(buffer.buffer, buffer.byteOffset, buffer.byteLength),
  ],
};
globalThis.__quench_verify_calls = () => {
  for (const callback of globalThis.__nodeCallChecks || []) {
    if (callback.calls !== callback.expected)
      throw new Error(
        `Callback ${callback.__quench_index}: expected ${callback.expected} calls, got ${callback.calls}`,
      );
  }
};
globalThis.__nodeTmpdir = {
  path: `/tmp/quench-node-${process.pid}`,
  hasEnoughSpace: (_bytes) => false,
  refresh: () => {
    try {
      globalThis.__quench_fs_mkdir(globalThis.__nodeTmpdir.path);
    } catch (_) {}
  },
  resolve: (name = "") =>
    globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name)),
  fileURL: (name = "") =>
    new globalThis.__nodeURL(
      `file://${globalThis.__nodePath.join(globalThis.__nodeTmpdir.path, String(name))}`,
    ),
};
class NodeEventEmitter {
  constructor() {
    this._events = {};
  }
  on(event, listener) {
    (this._events[event] ||= []).push(listener);
    return this;
  }
  addListener(event, listener) {
    return this.on(event, listener);
  }
  once(event, listener) {
    const wrapped = (...args) => {
      this.removeListener(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  emit(event, ...args) {
    const listeners = this._events[event] || [];
    listeners.slice().forEach((listener) => listener(...args));
    return listeners.length > 0;
  }
  removeListener(event, listener) {
    this._events[event] = (this._events[event] || []).filter(
      (item) => item !== listener,
    );
    return this;
  }
  off(event, listener) {
    return this.removeListener(event, listener);
  }
  removeAllListeners(event) {
    if (event === undefined) this._events = {};
    else delete this._events[event];
    return this;
  }
  listeners(event) {
    return (this._events[event] || []).slice();
  }
  listenerCount(event) {
    return (this._events[event] || []).length;
  }
}
globalThis.__nodeEventEmitter = NodeEventEmitter;
globalThis.__nodeEventEmitter.once = (emitter, event) =>
  new Promise((resolve) => emitter.once(event, (...args) => resolve(args)));
globalThis.__nodeEventEmitter.on = async function* (emitter, event) {
  const queue = [];
  let wake;
  emitter.on(event, (...args) => {
    queue.push(args);
    if (wake) {
      wake();
      wake = undefined;
    }
  });
  while (true) {
    if (!queue.length)
      await new Promise((resolve) => {
        wake = resolve;
      });
    yield queue.shift();
  }
};
class NodeReadable extends NodeEventEmitter {
  static from(iterable) {
    const stream = new NodeReadable();
    stream._chunks = Array.from(iterable);
    queueMicrotask(() => {
      for (const chunk of stream._chunks) stream.emit("data", chunk);
      stream.emit("end");
    });
    return stream;
  }
  pipe(destination) {
    this.on("data", (chunk) => destination.write(chunk));
    this.on("end", () => destination.end());
    return destination;
  }

  async *[Symbol.asyncIterator]() {
    for (const chunk of this._chunks || []) yield chunk;
  }
}
class NodeWritable extends NodeEventEmitter {
  write(chunk, encoding, callback) {
    if (typeof encoding === "function") callback = encoding;
    this.emit("data", chunk);
    if (callback) queueMicrotask(callback);
    return true;
  }
  end(chunk, encoding, callback) {
    if (chunk !== undefined) this.write(chunk, encoding);
    if (callback) callback();
    this.emit("finish");
  }
}
class NodeTransform extends NodeWritable {
  constructor(options = {}) {
    super();
    this._transform = options.transform;
  }
  write(chunk, encoding, callback) {
    if (this._transform)
      this._transform.call(this, chunk, encoding, () => callback && callback());
    else super.write(chunk, encoding, callback);
    return true;
  }
}
globalThis.__nodeStream = {
  Readable: NodeReadable,
  Writable: NodeWritable,
  Transform: NodeTransform,
  PassThrough: NodeTransform,
};
globalThis.__nodeFs = {
  constants: {
    F_OK: 0,
    R_OK: 4,
    W_OK: 2,
    X_OK: 1,
    O_APPEND: 1024,
    O_CREAT: 64,
    O_EXCL: 128,
    O_RDONLY: 0,
    O_RDWR: 2,
    O_SYNC: 1052672,
    O_DSYNC: 4194304,
    O_TRUNC: 512,
    O_WRONLY: 1,
    UV_DIRENT_UNKNOWN: 0,
    UV_DIRENT_FILE: 1,
    UV_DIRENT_DIR: 2,
    UV_DIRENT_LINK: 3,
    UV_DIRENT_FIFO: 4,
    UV_DIRENT_SOCKET: 5,
    UV_DIRENT_CHAR: 6,
    UV_DIRENT_BLOCK: 7,
    COPYFILE_EXCL: 1,
    COPYFILE_FICLONE: 2,
    COPYFILE_FICLONE_FORCE: 4,
    UV_FS_COPYFILE_EXCL: 1,
    UV_FS_COPYFILE_FICLONE: 2,
    UV_FS_COPYFILE_FICLONE_FORCE: 4,
  },
  existsSync: (value) => globalThis.__quench_fs_exists(nodePathValue(value)),
  mkdtempSync: (prefix) =>
    globalThis.__quench_fs_mkdtemp(nodePathValue(prefix)),
  readFileSync: (value, options) => {
    const path = nodePathValue(value);
    let hex;
    try {
      hex = globalThis.__quench_fs_read_hex(path);
    } catch (error) {
      const flag =
        typeof options === "object" && options ? options.flag : undefined;
      if (flag === "a" || flag === "a+") {
        globalThis.__quench_fs_write_hex(path, "");
        globalThis.__nodeModes[path] = 0o666 & ~process.umask();
        hex = "";
      } else throw error;
    }
    if (options === undefined || options === null)
      return NodeBuffer.from(hex, "hex");
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    if (
      options &&
      typeof options === "object" &&
      options.buffer !== undefined
    ) {
      const bytes = NodeBuffer.from(hex, "hex");
      const target =
        typeof options.buffer === "function"
          ? options.buffer(bytes.length)
          : options.buffer;
      if (!(target instanceof Uint8Array)) {
        const error = new TypeError('The "buffer" option must return a Buffer');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      target.set(bytes.subarray(0, target.length));
      return encoding
        ? target.toString(encoding)
        : target.subarray(0, bytes.length);
    }
    if (encoding === "hex" || encoding === "base64")
      return NodeBuffer.from(hex, "hex").toString(encoding);
    return globalThis.__quench_fs_read_file(path);
  },
  writeFileSync: (value, data, options = {}) => {
    if (
      options &&
      options.flush !== undefined &&
      typeof options.flush !== "boolean"
    ) {
      const error = new TypeError(
        'The "options.flush" property must be of type boolean',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path =
      typeof value === "number"
        ? globalThis.__nodeFdPaths[value]
        : nodePathValue(value);
    if (!path) {
      const error = new Error("EBADF");
      error.code = "EBADF";
      throw error;
    }
    let view =
      data instanceof Uint8Array
        ? data
        : ArrayBuffer.isView(data)
          ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
          : undefined;
    if (
      !view &&
      options &&
      options.encoding &&
      options.encoding !== "utf8" &&
      options.encoding !== "utf-8"
    )
      view = NodeBuffer.from(String(data), options.encoding);
    const hex = view
      ? NodeBuffer.from(view).toString("hex")
      : NodeBuffer.from(String(data)).toString("hex");
    if (options && options.flag === "a") {
      let existing = "";
      try {
        existing = globalThis.__quench_fs_read_hex(path);
      } catch (_) {}
      return globalThis.__quench_fs_write_hex(path, existing + hex);
    }
    const result = globalThis.__quench_fs_write_hex(path, hex);
    if (options && options.flush) {
      const fd = globalThis.__nodeFs.openSync(path, "r");
      globalThis.__nodeFs.fsyncSync(fd);
      globalThis.__nodeFs.closeSync(fd);
    }
    if (options && options.mode !== undefined)
      globalThis.__nodeModes[path] = Number(options.mode);
    return result;
  },
  openSync: (value, flags = "r", mode) => {
    const path = nodeFsPath(value);
    if (
      mode !== undefined &&
      mode !== null &&
      typeof mode !== "number" &&
      typeof mode !== "string"
    ) {
      const error = new TypeError('The "mode" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const flag = String(flags);
    if (!/^[wax]/.test(flag) && !globalThis.__quench_fs_access(path)) {
      const error = new Error(
        `ENOENT: no such file or directory, open '${path}'`,
      );
      error.code = "ENOENT";
      error.syscall = "open";
      error.path = path;
      throw error;
    }
    const fd = globalThis.__quench_fs_open(path, flag);
    globalThis.__nodeFdPaths[fd] = path;
    globalThis.__nodeFdPositions[fd] = 0;
    if (mode !== undefined && mode !== null)
      globalThis.__nodeModes[path] =
        typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
    return fd;
  },
  closeSync: (fd) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    delete globalThis.__nodeFdPaths[fd];
    delete globalThis.__nodeFdPositions[fd];
  },
  statSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try {
      kind = globalThis.__quench_fs_kind(path);
    } catch (error) {
      if (options && options.throwIfNoEntry === false) return undefined;
      throw error;
    }
    const file = kind === "file";
    const date = new Date();
    const stats = new globalThis.__nodeStats(file, kind === "directory", date);
    if (file) stats.size = globalThis.__quench_fs_read_hex(path).length / 2;
    stats.mode =
      globalThis.__nodeModes[path] || (file ? 0o666 & ~process.umask() : 0);
    return stats;
  },
  mkdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    if (
      options &&
      Object.prototype.hasOwnProperty.call(options, "recursive") &&
      typeof options.recursive !== "boolean"
    ) {
      const error = new TypeError(
        'The "options.recursive" property must be of type boolean.',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let targetKind;
    try {
      targetKind = globalThis.__quench_fs_kind(path);
    } catch (_) {
      targetKind = undefined;
    }
    if (targetKind === "file") {
      const error = new Error(`EEXIST: file already exists, mkdir '${path}'`);
      error.code = "EEXIST";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
    if (targetKind === "directory" && !(options && options.recursive)) {
      const error = new Error(`EEXIST: file already exists, mkdir '${path}'`);
      error.code = "EEXIST";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
    const parts = path.split("/").filter(Boolean);
    let prefix = path.startsWith("/") ? "" : ".";
    for (const part of parts.slice(0, -1)) {
      prefix += `/${part}`;
      let kind;
      try {
        kind = globalThis.__quench_fs_kind(prefix);
      } catch (_) {
        kind = undefined;
      }
      if (kind === "file") {
        const error = new Error(`ENOTDIR: not a directory, mkdir '${path}'`);
        error.code = "ENOTDIR";
        error.syscall = "mkdir";
        error.path = path;
        throw error;
      }
    }
    try {
      return globalThis.__quench_fs_mkdir(path);
    } catch (_) {
      const error = new Error(
        `ENOENT: no such file or directory, mkdir '${path}'`,
      );
      error.code = "ENOENT";
      error.syscall = "mkdir";
      error.path = path;
      throw error;
    }
  },
  readdirSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try {
      kind = globalThis.__quench_fs_kind(path);
    } catch (_) {
      kind = undefined;
    }
    if (kind === "file") {
      const error = new Error(`ENOTDIR: not a directory, scandir '${path}'`);
      error.code = "ENOTDIR";
      error.syscall = "scandir";
      error.path = path;
      throw error;
    }
    const entries = globalThis.__quench_fs_readdir(path).sort();
    if (!options || !options.withFileTypes) return entries;
    return entries.map((name) => {
      const dirent = new globalThis.__nodeFs.Dirent(
        name,
        (() => {
          try {
            return (
              globalThis.__quench_fs_kind(`${path}/${name}`) === "directory"
            );
          } catch (_) {
            return false;
          }
        })(),
      );
      dirent.parentPath = path;
      return dirent;
    });
  },
  rmdirSync: (value) => globalThis.__quench_fs_remove_dir(String(value)),
  renameSync: (from, to) =>
    globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to)),
  unlinkSync: (value) => globalThis.__quench_fs_unlink(String(value)),
  truncateSync: (value, length = 0) => {
    if (typeof length !== "number" || !Number.isFinite(length)) {
      const error = new TypeError('The "len" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(length)) {
      const error = new RangeError('The value of "len" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const path =
      typeof value === "number"
        ? globalThis.__nodeFdPaths[value]
        : nodeFsPath(value);
    if (!path) throw new Error("EBADF");
    return globalThis.__quench_fs_truncate(path, Math.max(0, Number(length)));
  },
  ftruncateSync: (fd, length = 0) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return globalThis.__nodeFs.truncateSync(fd, length);
  },
  fsyncSync: (fd) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(fd) || fd < 0) {
      const error = new RangeError('The value of "fd" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  },
  fdatasyncSync: (fd) => globalThis.__nodeFs.fsyncSync(fd),
  readSync: (
    fd,
    buffer,
    offset = 0,
    length = buffer.length,
    position = null,
  ) => {
    if (
      offset !== undefined &&
      offset !== null &&
      (Array.isArray(offset) ||
        (typeof offset !== "number" && typeof offset !== "object"))
    ) {
      const error = new TypeError('The "options" argument must be an object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      offset === null ||
      (typeof offset === "object" && !ArrayBuffer.isView(offset))
    ) {
      const options = offset || {};
      if (offset !== null && typeof offset !== "object") {
        const error = new TypeError('The "options" argument must be an object');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      offset = Number(options.offset || 0);
      length =
        options.length === undefined
          ? buffer.length - offset
          : Number(options.length);
      position = options.position === undefined ? null : options.position;
    }
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!(buffer instanceof Uint8Array)) {
      const error = new TypeError(
        'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (buffer.length === 0 && Number(length) > 0) {
      const error = new TypeError(
        "The argument 'buffer' is empty and cannot be written.",
      );
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (!Number.isInteger(offset) || offset < 0) {
      const error = new RangeError('The value of "offset" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(length) || length < 0) {
      const error = new RangeError('The value of "length" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (
      position !== null &&
      position !== undefined &&
      typeof position !== "number" &&
      typeof position !== "bigint"
    ) {
      const error = new TypeError(
        'The "position" argument must be of type number or bigint',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path = globalThis.__nodeFdPaths[fd];
    if (!path) {
      const error = new Error("EBADF");
      error.code = "EBADF";
      throw error;
    }
    const numericPosition =
      position === null || Number(position) < 0
        ? globalThis.__nodeFdPositions[fd] || 0
        : Number(position);
    const hex = globalThis.__quench_fs_read_range_hex(
      path,
      numericPosition,
      Number(length),
    );
    const bytes = NodeBuffer.from(hex, "hex");
    buffer.set(bytes.subarray(0, Number(length)), Number(offset));
    if (position === null || position === undefined)
      globalThis.__nodeFdPositions[fd] = numericPosition + bytes.length;
    return bytes.length;
  },
  readvSync: (fd, buffers, position = null) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      !Array.isArray(buffers) ||
      buffers.some((buffer) => !(buffer instanceof Uint8Array))
    ) {
      const error = new TypeError(
        'The "buffers" argument must be an array of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let total = 0;
    let at = position === null || position === undefined ? 0 : Number(position);
    for (const buffer of buffers) {
      if (buffer.length) {
        const count = globalThis.__nodeFs.readSync(
          fd,
          buffer,
          0,
          buffer.length,
          at,
        );
        total += count;
        at += count;
        if (count < buffer.length) break;
      }
    }
    return total;
  },
  writeSync: (
    fd,
    buffer,
    offset = 0,
    length = buffer.length - offset,
    position = null,
  ) => {
    if (typeof fd !== "number") {
      const error = new TypeError('The "fd" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof buffer === "string") buffer = NodeBuffer.from(buffer);
    if (!(buffer instanceof Uint8Array)) {
      const error = new TypeError(
        'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      !Number.isInteger(offset) ||
      offset < 0 ||
      !Number.isInteger(length) ||
      length < 0
    ) {
      const error = new RangeError("The write range is out of range");
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const path = globalThis.__nodeFdPaths[fd];
    if (!path) {
      const error = new Error("EBADF");
      error.code = "EBADF";
      throw error;
    }
    const bytes = buffer.subarray(offset, offset + length);
    const at =
      position === null || position === undefined
        ? globalThis.__nodeFdPositions[fd] || 0
        : Number(position);
    const existing = NodeBuffer.from(
      globalThis.__quench_fs_read_hex(path),
      "hex",
    );
    const output = NodeBuffer.alloc(
      Math.max(existing.length, at + bytes.length),
    );
    output.set(existing);
    output.set(bytes, at);
    globalThis.__quench_fs_write_hex(path, output.toString("hex"));
    if (position === null || position === undefined)
      globalThis.__nodeFdPositions[fd] = at + bytes.length;
    return bytes.length;
  },
  writevSync: (fd, buffers, position = null) => {
    if (
      !Array.isArray(buffers) ||
      buffers.some((buffer) => !(buffer instanceof Uint8Array))
    ) {
      const error = new TypeError(
        'The "buffers" argument must be an array of Buffer or Uint8Array',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return globalThis.__nodeFs.writeSync(fd, NodeBuffer.concat(buffers));
  },
  copyFileSync: (from, to, mode = 0) => {
    const source = nodeFsPath(from);
    const destination = nodeFsPath(to);
    if (typeof mode !== "number") {
      const error = new TypeError('The "mode" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if ((mode & ~7) !== 0) {
      const error = new RangeError('The value of "mode" is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return globalThis.__quench_fs_copy(source, destination);
  },
  appendFileSync: (value, data, options = {}) => {
    if (
      !(
        typeof data === "string" ||
        data instanceof NodeBuffer ||
        data instanceof Uint8Array
      )
    ) {
      const error = new TypeError(
        'The "data" argument must be of type string or an instance of Buffer',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path =
      typeof value === "number"
        ? globalThis.__nodeFdPaths[value]
        : nodeFsPath(value);
    if (!path) {
      const error = new Error("EBADF: bad file descriptor");
      error.code = "EBADF";
      throw error;
    }
    const result = globalThis.__quench_fs_append(
      path,
      data instanceof Uint8Array
        ? new NodeTextDecoder().decode(data)
        : String(data),
    );
    if (options && options.mode !== undefined)
      globalThis.__nodeModes[path] = Number(options.mode);
    return result;
  },
  accessSync: (value) => {
    if (typeof value === "number") {
      const error = new TypeError(
        'The "path" argument must be of type string or an instance of Buffer or URL',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const path = nodeFsPath(value);
    if (!globalThis.__quench_fs_access(path)) {
      const error = new Error(
        `ENOENT: no such file or directory, access '${path}'`,
      );
      error.code = "ENOENT";
      error.path = path;
      throw error;
    }
  },
  realpathSync: (value, options) => {
    const result = globalThis.__quench_fs_realpath(nodePathValue(value));
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
        ? NodeBuffer.from(result).toString(encoding)
        : result;
  },
  rmSync: (value, options = {}) => {
    const path = nodeFsPath(value);
    let kind;
    try {
      kind = globalThis.__quench_fs_kind(path);
    } catch (_) {
      return;
    }
    if (kind === "file") return globalThis.__quench_fs_unlink(path);
    if (kind === "directory" && options.recursive === false) {
      const error = new Error(
        `ERR_FS_EISDIR: illegal operation on a directory, rm '${path}'`,
      );
      error.code = "ERR_FS_EISDIR";
      error.path = path;
      throw error;
    }
    return globalThis.__quench_fs_remove_dir(path);
  },
  chmodSync: (value, mode) => {
    const path = nodeFsPath(value);
    globalThis.__quench_fs_chmod(
      path,
      typeof mode === "string" ? parseInt(mode, 8) : Number(mode),
    );
    globalThis.__nodeModes[path] =
      typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  },
  symlinkSync: (target, link, type) => {
    if (
      (typeof target !== "string" && !(target instanceof Uint8Array)) ||
      (typeof link !== "string" && !(link instanceof Uint8Array))
    ) {
      const error = new TypeError(
        'The "target" and "path" arguments must be strings or Buffer',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      type !== undefined &&
      type !== "file" &&
      type !== "dir" &&
      type !== "junction"
    ) {
      const error = new TypeError('The "type" argument is invalid');
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return globalThis.__quench_fs_symlink(nodeFsPath(target), nodeFsPath(link));
  },
  linkSync: (existing, link) => {
    if (
      (typeof existing !== "string" && !(existing instanceof Uint8Array)) ||
      (typeof link !== "string" && !(link instanceof Uint8Array))
    ) {
      const error = new TypeError(
        'The "path" argument must be of type string or an instance of Buffer or URL',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return globalThis.__quench_fs_link(nodeFsPath(existing), nodeFsPath(link));
  },
  readlinkSync: (value, options) => {
    const result = globalThis.__quench_fs_readlink(nodeFsPath(value));
    const encoding =
      typeof options === "string" ? options : options && options.encoding;
    return encoding === "buffer"
      ? NodeBuffer.from(result)
      : encoding
        ? NodeBuffer.from(result).toString(encoding)
        : result;
  },
};
globalThis.__nodeFs.truncate = (value, length, callback) => {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof length !== "number" || !Number.isFinite(length)) {
    const error = new TypeError('The "len" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(length)) {
    const error = new RangeError('The value of "len" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  const path =
    typeof value === "number"
      ? globalThis.__nodeFdPaths[value]
      : nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__quench_fs_truncate(path, Number(length));
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.ftruncate = (fd, length = 0, callback) => {
  if (typeof length === "function") {
    callback = length;
    length = 0;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.ftruncateSync(fd, length);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.access = (value, mode, callback) => {
  if (typeof mode === "function") callback = mode;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof value === "number") {
    const error = new TypeError(
      'The "path" argument must be of type string or an instance of Buffer or URL',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.accessSync(path, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.fsync = (fd, callback) => {
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.fsyncSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.fdatasync = (fd, callback) => {
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.fdatasyncSync(fd);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.read = (fd, buffer, offset, length, position, callback) => {
  if (typeof buffer === "function" || buffer === undefined) {
    callback = buffer;
    buffer = NodeBuffer.alloc(16384);
    offset = 0;
    length = buffer.length;
    position = null;
  } else if (buffer === null) {
    callback = offset;
    buffer = NodeBuffer.alloc(16384);
    offset = 0;
    length = buffer.length;
    position = null;
  } else if (typeof buffer === "object" && !ArrayBuffer.isView(buffer)) {
    const options = buffer;
    callback = offset;
    buffer =
      options.buffer ||
      NodeBuffer.alloc(
        options.length === undefined ? 16384 : Number(options.length),
      );
    offset = options.offset == null ? 0 : Number(options.offset);
    length =
      options.length === undefined
        ? buffer.length - offset
        : Number(options.length);
    position = options.position === undefined ? null : options.position;
  } else if (typeof offset === "function") {
    callback = offset;
    offset = 0;
    length = buffer.length;
    position = null;
  } else if (
    typeof offset === "object" ||
    offset === null ||
    offset === undefined
  ) {
    const options = offset || {};
    callback = length;
    offset = Number(options.offset || 0);
    length =
      options.length === undefined
        ? buffer.length - offset
        : Number(options.length);
    position = options.position === undefined ? null : options.position;
  } else if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (buffer.length === 0 && Number(length) > 0) {
    const error = new TypeError(
      "The argument 'buffer' is empty and cannot be written.",
    );
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!(buffer instanceof Uint8Array)) {
    const error = new TypeError(
      'The "buffer" argument must be an instance of Buffer, TypedArray, or DataView',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Number.isInteger(offset) ||
    offset < 0 ||
    !Number.isInteger(length) ||
    length < 0
  ) {
    const error = new RangeError("The read range is out of range");
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  if (
    position !== null &&
    position !== undefined &&
    typeof position !== "number" &&
    typeof position !== "bigint"
  ) {
    const error = new TypeError(
      'The "position" argument must be of type number or bigint',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      const count = globalThis.__nodeFs.readSync(
        fd,
        buffer,
        offset,
        length,
        position,
      );
      callback(null, count, buffer);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.readv = (fd, buffers, position, callback) => {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "buffers" argument must be an array of Buffer or Uint8Array',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.readvSync(fd, buffers, position),
        buffers,
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.write = (
  fd,
  buffer,
  offset,
  length,
  position,
  callback,
) => {
  if (
    typeof buffer === "object" &&
    buffer !== null &&
    !ArrayBuffer.isView(buffer)
  ) {
    const options = buffer;
    callback = offset;
    buffer = options.buffer;
    offset = options.offset || 0;
    length =
      options.length === undefined
        ? buffer && buffer.length - offset
        : options.length;
    position = options.position;
  } else if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    typeof fd !== "number" ||
    !(typeof buffer === "string" || buffer instanceof Uint8Array)
  ) {
    const error = new TypeError("Invalid write arguments");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.writeSync(fd, buffer, offset, length, position),
        buffer,
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.writev = (fd, buffers, position, callback) => {
  if (typeof position === "function") {
    callback = position;
    position = null;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    !Array.isArray(buffers) ||
    buffers.some((buffer) => !(buffer instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "buffers" argument must be an array of Buffer or Uint8Array',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      callback(
        null,
        globalThis.__nodeFs.writevSync(fd, buffers, position),
        buffers,
      );
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeModes = {};
globalThis.__nodeFdPaths = {};
globalThis.__nodeFdPositions = {};
const nodeMode = (mode) => {
  const value = typeof mode === "string" ? parseInt(mode, 8) : Number(mode);
  if (!Number.isFinite(value) || value < 0 || value > 0xffffffff) {
    const error = new RangeError('The value of "mode" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return value;
};
globalThis.__nodeFs.fchmodSync = (fd, mode) => {
  if (!Number.isInteger(fd) || fd < 0 || fd > 0x7fffffff) {
    const error = new RangeError('The value of "fd" is out of range');
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  const value = nodeMode(mode);
  if (globalThis.__nodeFdPaths[fd])
    globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[fd], value);
};
globalThis.__nodeFs.fchmod = (fd, mode, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  globalThis.__nodeFs.fchmodSync(fd, mode);
  queueMicrotask(() => callback(null));
};
globalThis.__nodeFs.statfsSync = (value, options = {}) => {
  const path = nodeFsPath(value);
  if (!globalThis.__quench_fs_access(path)) throw new Error("ENOENT");
  const values = {
    type: 0,
    bsize: 4096,
    frsize: 4096,
    blocks: 1,
    bfree: 1,
    bavail: 1,
    files: 1,
    ffree: 1,
  };
  if (options && options.bigint)
    Object.keys(values).forEach((key) => {
      values[key] = BigInt(values[key]);
    });
  return values;
};
globalThis.__nodeFs.statfs = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.statfsSync(path, options));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.symlink = (target, link, type, callback) => {
  if (typeof type === "function") {
    callback = type;
    type = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    (typeof target !== "string" && !(target instanceof Uint8Array)) ||
    (typeof link !== "string" && !(link instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "target" and "path" arguments must be strings or Buffer',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    type !== undefined &&
    type !== "file" &&
    type !== "dir" &&
    type !== "junction"
  ) {
    const error = new TypeError('The "type" argument is invalid');
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
  const source = nodePathValue(target);
  const destination = nodeFsPath(link);
  queueMicrotask(() => {
    try {
      globalThis.__quench_fs_symlink(source, destination);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.readlink = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.readlinkSync(path, options));
    } catch (error) {
      callback(error);
      return;
    }
  });
};
globalThis.__nodeFs.link = (existing, link, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    (typeof existing !== "string" && !(existing instanceof Uint8Array)) ||
    (typeof link !== "string" && !(link instanceof Uint8Array))
  ) {
    const error = new TypeError(
      'The "path" argument must be of type string or an instance of Buffer or URL',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.linkSync(existing, link);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.chmod = (value, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.chmodSync(path, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.appendFile = (value, data, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.appendFileSync(value, data, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rmdir = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.rmdirSync(path);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rm = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.rmSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.rename = (from, to, callback) => {
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const source = nodeFsPath(from);
  const destination = nodeFsPath(to);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.renameSync(source, destination);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.copyFile = (from, to, mode, callback) => {
  if (typeof mode === "function") {
    callback = mode;
    mode = 0;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const source = nodeFsPath(from);
  const destination = nodeFsPath(to);
  if (typeof mode !== "number") {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.copyFileSync(source, destination, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.realpath = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.realpathSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.realpathSync.native = globalThis.__nodeFs.realpathSync;
globalThis.__nodeFs.realpath.native = globalThis.__nodeFs.realpath;
globalThis.__nodeStats = function Stats(
  file = false,
  directory = false,
  date = new Date(),
) {
  if (!(date instanceof Date)) date = new Date(Number(date) || 0);
  this.dev = 0;
  this.mode = 0;
  this.nlink = 1;
  this.uid = 0;
  this.gid = 0;
  this.rdev = 0;
  this.blksize = 4096;
  this.ino = 0;
  this.size = 0;
  this.blocks = 0;
  this.atime = date;
  this.mtime = date;
  this.ctime = date;
  this.birthtime = date;
  this.atimeMs = date.getTime();
  this.mtimeMs = date.getTime();
  this.ctimeMs = date.getTime();
  this.birthtimeMs = date.getTime();
  this._file = file;
  this._directory = directory;
};
globalThis.__nodeStats.prototype.isFile = function () {
  return this._file;
};
globalThis.__nodeStats.prototype.isDirectory = function () {
  return this._directory;
};
globalThis.__nodeStats.prototype.isSocket = function () {
  return false;
};
globalThis.__nodeStats.prototype.isBlockDevice = function () {
  return false;
};
globalThis.__nodeStats.prototype.isCharacterDevice = function () {
  return false;
};
globalThis.__nodeStats.prototype.isFIFO = function () {
  return false;
};
globalThis.__nodeStats.prototype.isSymbolicLink = function () {
  return this._symlink === true;
};
globalThis.__nodeFs.Dirent = class Dirent {
  constructor(name, type = 1) {
    this.name = name;
    this._type = type === true ? 2 : type === false ? 1 : type;
  }
  isFile() {
    return this._type === 1;
  }
  isDirectory() {
    return this._type === 2;
  }
  isSymbolicLink() {
    return this._type === 3;
  }
  isFIFO() {
    return this._type === 4;
  }
  isSocket() {
    return this._type === 5;
  }
  isCharacterDevice() {
    return this._type === 6;
  }
  isBlockDevice() {
    return this._type === 7;
  }
};
globalThis.__nodeFs.Dir = class Dir {
  constructor(path) {
    this.path = path;
    this._entries = globalThis.__nodeFs.readdirSync(path, {
      withFileTypes: true,
    });
    this._index = 0;
    this._closed = false;
  }
  readSync() {
    if (this._closed) {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      throw error;
    }
    return this._entries[this._index++] || null;
  }
  closeSync() {
    if (this._closed) {
      const error = new Error("Directory handle was closed");
      error.code = "ERR_DIR_CLOSED";
      throw error;
    }
    this._closed = true;
  }
  read(callback) {
    if (typeof callback !== "function")
      throw new TypeError('The "callback" argument must be of type function');
    queueMicrotask(() => {
      try {
        callback(null, this.readSync());
      } catch (error) {
        callback(error);
      }
    });
  }
  close(callback) {
    if (typeof callback !== "function")
      throw new TypeError('The "callback" argument must be of type function');
    queueMicrotask(() => {
      try {
        this.closeSync();
        callback(null);
      } catch (error) {
        callback(error);
      }
    });
  }
};
globalThis.__nodeFs.opendirSync = (value) =>
  new globalThis.__nodeFs.Dir(nodeFsPath(value));
globalThis.__nodeFs.opendir = (value, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, new globalThis.__nodeFs.Dir(path));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.lstatSync = (value) => {
  const path = nodeFsPath(value);
  const kind = globalThis.__quench_fs_link_kind(path);
  const stats = new globalThis.__nodeStats(
    kind === "file",
    kind === "directory",
    new Date(),
  );
  stats._symlink = kind === "symlink";
  stats.mode = globalThis.__nodeModes[path] || 0;
  return stats;
};
globalThis.__nodeFs.stat = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.statSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.lstat = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.lstatSync(path));
    } catch (error) {
      callback(error);
      return;
    }
  });
};
globalThis.__nodeFs.fstatSync = (fd) => {
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  return globalThis.__nodeFs.statSync(globalThis.__nodeFdPaths[fd] || ".");
};
globalThis.__nodeFs.fstat = (fd, options, callback) => {
  if (typeof options === "function") callback = options;
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.fstatSync(fd);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.Stats = globalThis.__nodeStats;
globalThis.__nodeFs.close = (fd, callback) => {
  if (typeof fd !== "number") {
    const error = new TypeError('The "fd" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (typeof callback !== "function") {
    const error = new TypeError(
      'The "callback" argument must be of type function',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => callback(null));
};
class NodeAbortSignal {
  constructor() {
    this.aborted = false;
    this._listeners = [];
  }
  addEventListener(event, listener) {
    if (event === "abort") this._listeners.push(listener);
  }
  removeEventListener(event, listener) {
    this._listeners = this._listeners.filter((item) => item !== listener);
  }
  static abort() {
    const signal = new NodeAbortSignal();
    signal.aborted = true;
    return signal;
  }
}
class NodeAbortController {
  constructor() {
    this.signal = new NodeAbortSignal();
  }
  abort() {
    this.signal.aborted = true;
    this.signal._listeners.slice().forEach((listener) => listener());
  }
}
globalThis.AbortSignal = NodeAbortSignal;
globalThis.AbortController = NodeAbortController;
globalThis.__nodeFs.open = (value, flags, mode, callback) => {
  if (typeof flags === "function") {
    callback = flags;
    flags = "r";
    mode = undefined;
  } else if (typeof mode === "function") {
    callback = mode;
    mode = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    mode !== undefined &&
    mode !== null &&
    typeof mode !== "number" &&
    typeof mode !== "string"
  ) {
    const error = new TypeError('The "mode" argument must be of type number');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let fd;
    try {
      fd = globalThis.__nodeFs.openSync(path, flags, mode);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, fd);
  });
};
globalThis.__nodeFs.readdir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    let result;
    try {
      result = globalThis.__nodeFs.readdirSync(path, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, result);
  });
};
globalThis.__nodeFs.mkdir = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = {};
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (
    options &&
    Object.prototype.hasOwnProperty.call(options, "recursive") &&
    typeof options.recursive !== "boolean"
  ) {
    const error = new TypeError(
      'The "options.recursive" property must be of type boolean.',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const path = nodeFsPath(value);
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.mkdirSync(path, options);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.readFile = (value, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (
    options &&
    options.signal !== undefined &&
    !(options.signal instanceof NodeAbortSignal)
  ) {
    const error = new TypeError('The "signal" option must be an AbortSignal');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  queueMicrotask(() => {
    if (options && options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      callback(error);
      return;
    }
    let data;
    try {
      data = globalThis.__nodeFs.readFileSync(value, options);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null, data);
  });
};
globalThis.__nodeFs.mkdtemp = (prefix, options, callback) => {
  if (typeof options === "function") callback = options;
  queueMicrotask(() => {
    try {
      callback(null, globalThis.__nodeFs.mkdtempSync(prefix));
    } catch (error) {
      callback(error);
    }
  });
};
globalThis.__nodeFs.writeFile = (value, data, options, callback) => {
  if (typeof options === "function") {
    callback = options;
    options = undefined;
  }
  if (typeof callback !== "function")
    throw new TypeError('The "callback" argument must be of type function');
  if (options && options.signal && options.signal.aborted) {
    queueMicrotask(() => {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      callback(error);
    });
    return;
  }
  queueMicrotask(() => {
    try {
      globalThis.__nodeFs.writeFileSync(value, data);
    } catch (error) {
      callback(error);
      return;
    }
    callback(null);
  });
};
globalThis.__nodeFs.promises = {
  open: (value, flags = "r", mode) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.open(value, flags, mode, (error, fd) =>
        error
          ? reject(error)
          : resolve({
              fd,
              close: () => Promise.resolve(),
              read: (buffer, offset, length, position) =>
                Promise.resolve().then(() => {
                  let target = buffer;
                  let start = offset;
                  let size = length;
                  let at = position;
                  if (offset && typeof offset === "object") {
                    const options = offset;
                    target = options.buffer || NodeBuffer.alloc(16384);
                    start = options.offset == null ? 0 : options.offset;
                    size =
                      options.length === undefined
                        ? target.length - start
                        : options.length;
                    at = options.position;
                  }
                  if (target.length === 0 && Number(size) > 0) {
                    const error = new TypeError("The buffer is empty");
                    error.code = "ERR_INVALID_ARG_VALUE";
                    throw error;
                  }
                  const bytesRead = globalThis.__nodeFs.readSync(
                    fd,
                    target,
                    start || 0,
                    size === undefined ? target.length : size,
                    at === undefined ? null : at,
                  );
                  return { bytesRead, buffer: target };
                }),
            }),
      ),
    ),
  readFile: (value, options) =>
    value && typeof value === "object" && typeof value.fd === "number"
      ? value.readFile(options)
      : new Promise((resolve, reject) =>
          globalThis.__nodeFs.readFile(value, options, (error, data) =>
            error ? reject(error) : resolve(data),
          ),
        ),
  writeFile: (value, data, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.writeFile(value, data, options, (error) =>
        error ? reject(error) : resolve(),
      ),
    ),
  appendFile: (value, data, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.appendFile(value, data, options, (error) =>
        error ? reject(error) : resolve(),
      ),
    ),
  access: (value, mode) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.access(value, mode, (error) =>
        error ? reject(error) : resolve(),
      ),
    ),
  truncate: (value, length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.truncateSync(value, length),
    ),
  ftruncate: (fd, length = 0) =>
    Promise.resolve().then(() => globalThis.__nodeFs.ftruncateSync(fd, length)),
  fsync: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(fd)),
  fdatasync: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(fd)),
  rm: (value, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.rm(value, options, (error) =>
        error ? reject(error) : resolve(),
      ),
    ),
  opendir: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.opendirSync(value)),
  symlink: (target, link, type) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.symlink(target, link, type, (error) =>
        error ? reject(error) : resolve(),
      ),
    ),
  readlink: (value, options) =>
    new Promise((resolve, reject) =>
      globalThis.__nodeFs.readlink(value, options, (error, result) =>
        error ? reject(error) : resolve(result),
      ),
    ),
  realpath: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.realpathSync(value, options),
    ),
  fstat: (fd) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fstatSync(fd)),
  fchmod: (fd, mode) =>
    Promise.resolve().then(() => globalThis.__nodeFs.fchmodSync(fd, mode)),
  chmod: (value, mode) =>
    Promise.resolve().then(() => globalThis.__nodeFs.chmodSync(value, mode)),
  rename: (from, to) =>
    Promise.resolve().then(() =>
      globalThis.__quench_fs_rename(nodeFsPath(from), nodeFsPath(to)),
    ),
  unlink: (value) =>
    Promise.resolve().then(() =>
      globalThis.__quench_fs_unlink(nodeFsPath(value)),
    ),
  copyFile: (from, to, mode = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.copyFileSync(from, to, mode),
    ),
  rmdir: (value, options) =>
    Promise.resolve().then(() => globalThis.__nodeFs.rmdirSync(value, options)),
  mkdtemp: (prefix) =>
    Promise.resolve().then(() => globalThis.__nodeFs.mkdtempSync(prefix)),
  readv: (fd, buffers, position) =>
    Promise.resolve().then(() => {
      const bytesRead = globalThis.__nodeFs.readvSync(fd, buffers, position);
      return { bytesRead, buffers };
    }),
  writev: (fd, buffers, position) =>
    Promise.resolve().then(() => {
      const bytesWritten = globalThis.__nodeFs.writevSync(
        fd,
        buffers,
        position,
      );
      return { bytesWritten, buffers };
    }),
  mkdir: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.mkdirSync(value)),
  readdir: (value, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.readdirSync(value, options),
    ),
  stat: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.statSync(value)),
  lstat: (value) =>
    Promise.resolve().then(() => globalThis.__nodeFs.lstatSync(value)),
  link: (existing, link) =>
    Promise.resolve().then(() => globalThis.__nodeFs.linkSync(existing, link)),
};
const __nodePromiseOpen = globalThis.__nodeFs.promises.open;
globalThis.__nodeFs.promises.open = async (...args) => {
  const handle = await __nodePromiseOpen(...args);
  handle.write = (buffer, offset, length, position) =>
    Promise.resolve().then(() => {
      const start =
        typeof offset === "object" ? offset.offset || 0 : offset || 0;
      const source =
        typeof offset === "object" ? offset.buffer || offset : buffer;
      const size =
        typeof offset === "object"
          ? offset.length === undefined
            ? source.length - start
            : offset.length
          : length === undefined
            ? source.length - start
            : length;
      const at = typeof offset === "object" ? offset.position : position;
      return {
        bytesWritten: globalThis.__nodeFs.writeSync(
          handle.fd,
          source,
          start,
          size,
          at === undefined ? null : at,
        ),
        buffer: source,
      };
    });
  handle.readv = (buffers, position) =>
    Promise.resolve().then(() => ({
      bytesRead: globalThis.__nodeFs.readvSync(handle.fd, buffers, position),
      buffers,
    }));
  handle.writev = (buffers, position) =>
    Promise.resolve().then(() => ({
      bytesWritten: globalThis.__nodeFs.writevSync(
        handle.fd,
        buffers,
        position,
      ),
      buffers,
    }));
  handle.truncate = (length = 0) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.ftruncateSync(handle.fd, length),
    );
  handle.stat = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.fstatSync(handle.fd));
  handle.sync = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.fsyncSync(handle.fd));
  handle.datasync = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.fdatasyncSync(handle.fd));
  handle.chmod = (mode) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.chmodSync(globalThis.__nodeFdPaths[handle.fd], mode),
    );
  handle.readFile = (options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.readFileSync(handle.fd, options),
    );
  handle.writeFile = async (data, options) => {
    if (options && options.signal)
      await new Promise((resolve) => queueMicrotask(resolve));
    if (options && options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
    if (data && data._chunks) data = data._chunks;
    if (
      data &&
      typeof data !== "string" &&
      !(data instanceof Uint8Array) &&
      !(data instanceof ArrayBuffer) &&
      typeof data[Symbol.asyncIterator] === "function"
    ) {
      const chunks = [];
      for await (const chunk of data) chunks.push(chunk);
      data = chunks;
    }
    if (
      data &&
      typeof data !== "string" &&
      !(data instanceof Uint8Array) &&
      !ArrayBuffer.isView(data) &&
      typeof data[Symbol.iterator] === "function"
    ) {
      const chunks = [];
      for (const chunk of data) {
        if (
          typeof chunk !== "string" &&
          !(chunk instanceof Uint8Array) &&
          !ArrayBuffer.isView(chunk)
        ) {
          const error = new TypeError(
            'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView',
          );
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
        chunks.push(chunk);
      }
      data =
        chunks.length === 1
          ? chunks[0]
          : NodeBuffer.concat(
              chunks.map((chunk) =>
                typeof chunk === "string"
                  ? NodeBuffer.from(
                      chunk,
                      typeof options === "string"
                        ? options
                        : options && options.encoding,
                    )
                  : NodeBuffer.from(chunk),
              ),
            );
    }
    if (
      typeof data !== "string" &&
      !(data instanceof Uint8Array) &&
      !ArrayBuffer.isView(data) &&
      !(data instanceof ArrayBuffer)
    ) {
      const error = new TypeError(
        'The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return globalThis.__nodeFs.writeFileSync(
      handle.fd,
      data,
      typeof options === "string" ? { encoding: options } : options,
    );
  };
  handle.appendFile = (data, options) =>
    Promise.resolve().then(() =>
      globalThis.__nodeFs.appendFileSync(handle.fd, data, options),
    );
  handle.close = () =>
    Promise.resolve().then(() => globalThis.__nodeFs.closeSync(handle.fd));
  return handle;
};
const __nodeOpenWithFilePosition = globalThis.__nodeFs.promises.open;
globalThis.__nodeFs.promises.open = async (...args) => {
  const handle = await __nodeOpenWithFilePosition(...args);
  const previousWriteFile = handle.writeFile;
  const previousReadFile = handle.readFile;
  handle.pull = (transformOrOptions, maybeOptions) => {
    if (!globalThis.__nodeFdPaths[handle.fd] || handle._pullLocked) {
      const error = new Error("The file handle is not in a valid state");
      error.code = "ERR_INVALID_STATE";
      throw error;
    }
    handle._pullLocked = true;
    const transform =
      typeof transformOrOptions === "function" ? transformOrOptions : undefined;
    const options = transform ? maybeOptions || {} : transformOrOptions || {};
    if (
      options.autoClose !== undefined &&
      typeof options.autoClose !== "boolean"
    ) {
      const error = new TypeError(
        'The "autoClose" option must be of type boolean',
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (
      options.signal !== undefined &&
      (!options.signal || typeof options.signal.aborted !== "boolean")
    ) {
      const error = new TypeError('The "signal" option must be an AbortSignal');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    for (const [name, value] of [
      ["start", options.start],
      ["limit", options.limit],
      ["chunkSize", options.chunkSize],
    ]) {
      if (value === undefined) continue;
      if (typeof value !== "number" || !Number.isFinite(value)) {
        const error = new TypeError(
          `The "${name}" option must be of type number`,
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isInteger(value) || value < 0) {
        const error = new RangeError(`The value of "${name}" is out of range`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    }
    const source = globalThis.__nodeFs.readFileSync(handle.fd);
    const start =
      options.start === undefined
        ? globalThis.__nodeFdPositions[handle.fd] || 0
        : Number(options.start);
    const end =
      options.limit === undefined
        ? source.length
        : Math.min(source.length, start + Number(options.limit));
    const chunkSize =
      options.chunkSize === undefined ? 128 * 1024 : Number(options.chunkSize);
    const batches = [];
    for (let offset = start; offset < end; offset += chunkSize)
      batches.push([
        source.subarray(offset, Math.min(end, offset + chunkSize)),
      ]);
    if (start >= end) batches.push([]);
    return {
      async *[Symbol.asyncIterator]() {
        try {
          if (options.signal && options.signal.aborted) {
            const error = new Error("The operation was aborted");
            error.name = "AbortError";
            throw error;
          }
          for (const batch of batches)
            yield transform ? transform(batch) : batch;
          globalThis.__nodeFdPositions[handle.fd] = end;
          if (options.autoClose) await handle.close();
        } finally {
          handle._pullLocked = false;
        }
      },
    };
  };
  handle.writeFile = async (data, options) => {
    if (typeof data === "string")
      data = NodeBuffer.from(
        data,
        options && options.encoding ? options.encoding : "utf8",
      );
    if (data instanceof ArrayBuffer) data = new Uint8Array(data);
    if (data instanceof Uint8Array || ArrayBuffer.isView(data)) {
      const view =
        data instanceof Uint8Array
          ? data
          : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      globalThis.__nodeFs.writeSync(handle.fd, view, 0, view.length, null);
      return;
    }
    return previousWriteFile(data, options);
  };
  handle.readFile = async (options) => {
    if (options && options.signal)
      await new Promise((resolve) => queueMicrotask(resolve));
    if (options && options.signal && options.signal.aborted) {
      const error = new Error("The operation was aborted");
      error.name = "AbortError";
      error.code = "ABORT_ERR";
      throw error;
    }
    return previousReadFile(options);
  };
  return handle;
};
globalThis.__nodeOs = {
  EOL: "\n",
  platform: () => process.platform,
  arch: () => process.arch,
  tmpdir: () => globalThis.__quench_tmpdir,
  homedir: () => globalThis.__quench_homedir,
  type: () => "Quench",
  endianness: () => "LE",
  hostname: () => globalThis.__quench_hostname,
  cpus: () =>
    Array.from({ length: globalThis.__quench_cpu_count }, () => ({
      model: "unknown",
      speed: 0,
      times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 },
    })),
  userInfo: () => ({ username: "", homedir: "/" }),
  constants: {
    signals: { SIGTERM: 15, SIGINT: 2 },
    errno: { ENOENT: -2, EACCES: -13 },
  },
};
globalThis.__nodeUtil = {
  TextEncoder: globalThis.TextEncoder,
  TextDecoder: globalThis.TextDecoder,
  promisify:
    (fn) =>
    (...args) =>
      new Promise((resolve, reject) =>
        fn(...args, (error, ...values) =>
          error
            ? reject(error)
            : resolve(values.length > 1 ? values : values[0]),
        ),
      ),
  format: (...args) => {
    if (!args.length) return "";
    const numeric = (value) => {
      const rendered = String(value);
      if (!globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator)
        return rendered;
      const [mantissa, exponent] = rendered.split("e");
      const sign = mantissa.startsWith("-") ? "-" : "";
      const unsigned = sign ? mantissa.slice(1) : mantissa;
      const [whole, fraction] = unsigned.split(".");
      const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, "_");
      return `${sign}${grouped}${fraction === undefined ? "" : `.${fraction}`}${exponent === undefined ? "" : `e${exponent}`}`;
    };
    const inspect = (value) => {
      if (value === null) return "null";
      if (value === undefined) return "undefined";
      if (typeof value === "string") return value;
      if (typeof value === "symbol") return String(value);
      if (Array.isArray(value))
        return value.length ? `[ ${value.map(inspect).join(", ")} ]` : "[]";
      if (typeof value === "object") {
        const entries = Object.keys(value).map(
          (key) => `${key}: ${inspect(value[key])}`,
        );
        return `{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
      }
      return String(value);
    };
    const stringValue = (value) => {
      if (value && typeof value === "object") {
        if (
          typeof value.toString === "function" &&
          value.toString !== Object.prototype.toString
        ) {
          try {
            return value.toString();
          } catch (_) {}
        }
        if (Array.isArray(value)) return `[ ${value.map(inspect).join(", ")} ]`;
        const entries = Object.keys(value).map(
          (key) =>
            `${key}: ${Array.isArray(value[key]) ? "[Array]" : inspect(value[key])}`,
        );
        return `{${entries.length ? ` ${entries.join(", ")} ` : ""}}`;
      }
      return String(value);
    };
    if (typeof args[0] !== "string") return args.map(inspect).join(" ");
    let index = 1;
    return (
      args[0].replace(/%[sdifjo%]/g, (token) => {
        if (token === "%%") return "%";
        if (index >= args.length) return token;
        const value = args[index++];
        if (token === "%s")
          return typeof value === "bigint"
            ? `${numeric(value)}n`
            : typeof value === "number"
              ? numeric(value)
              : stringValue(value);
        if (token === "%d" || token === "%f") {
          if (typeof value === "bigint" && token === "%d")
            return `${numeric(value)}n`;
          if (typeof value === "symbol") return "NaN";
          let number;
          try {
            number = Number(value);
          } catch (_) {
            number = NaN;
          }
          return Object.is(number, -0) ? "-0" : numeric(number);
        }
        if (token === "%i") {
          if (typeof value === "bigint") return `${numeric(value)}n`;
          let number;
          try {
            number = Number.parseInt(value, 10);
          } catch (_) {
            number = NaN;
          }
          return Object.is(number, -0) ? "-0" : numeric(number);
        }
        if (token === "%j") return JSON.stringify(value);
        return token === "%o" ? inspect(value) : String(value);
      }) +
      args
        .slice(index)
        .map((value) => ` ${inspect(value)}`)
        .join("")
    );
  },
  inspect: (value) => {
    if (value instanceof NodeBuffer) {
      const custom = value[Symbol.for("nodejs.util.inspect.custom")];
      const properties = Object.keys(value).map((key) => {
        const item = value[key];
        if (item instanceof Uint8Array)
          return `${key}: ${item.constructor.name}(${item.length}) []`;
        return `${key}: ${item === undefined ? "undefined" : String(item)}`;
      });
      const rendered =
        typeof custom === "function"
          ? custom.call(value)
          : `<Buffer ${Array.from(value).join(" ")}>`;
      return `${rendered}${properties.length ? `, ${properties.join(", ")}` : ""}`;
    }
    if (
      value &&
      typeof value[Symbol.for("nodejs.util.inspect.custom")] === "function"
    )
      return value[Symbol.for("nodejs.util.inspect.custom")]();
    return JSON.stringify(value);
  },
  types: {
    isDate: (value) => value instanceof Date,
    isPromise: (value) => value instanceof Promise,
    isBooleanObject: (value) => value instanceof Boolean,
    isNumberObject: (value) => value instanceof Number,
    isStringObject: (value) => value instanceof String,
    isSymbolObject: (value) =>
      Object.prototype.toString.call(value) === "[object Symbol]",
    isBigIntObject: (value) =>
      Object.prototype.toString.call(value) === "[object BigInt]",
    isNativeError: (value) => value instanceof Error,
    isRegExp: (value) => value instanceof RegExp,
    isAsyncFunction: (value) =>
      Object.prototype.toString.call(value) === "[object AsyncFunction]",
    isGeneratorFunction: (value) =>
      Object.prototype.toString.call(value) === "[object GeneratorFunction]",
    isGeneratorObject: (value) =>
      Object.prototype.toString.call(value) === "[object Generator]",
    isMap: (value) => value instanceof Map,
    isSet: (value) => value instanceof Set,
    isWeakMap: (value) => value instanceof WeakMap,
    isWeakSet: (value) => value instanceof WeakSet,
    isArrayBuffer: (value) => value instanceof ArrayBuffer,
    isSharedArrayBuffer: (value) => value instanceof SharedArrayBuffer,
    isAnyArrayBuffer: (value) =>
      value instanceof ArrayBuffer || value instanceof SharedArrayBuffer,
    isArrayBufferView: (value) => ArrayBuffer.isView(value),
    isDataView: (value) => value instanceof DataView,
    isBoxedPrimitive: (value) =>
      value instanceof Boolean ||
      value instanceof Number ||
      value instanceof String ||
      Object.prototype.toString.call(value) === "[object Symbol]" ||
      Object.prototype.toString.call(value) === "[object BigInt]",
    isArgumentsObject: (value) =>
      Object.prototype.toString.call(value) === "[object Arguments]",
    isMapIterator: (value) =>
      Object.prototype.toString.call(value) === "[object Map Iterator]",
    isSetIterator: (value) =>
      Object.prototype.toString.call(value) === "[object Set Iterator]",
    isTypedArray: (value) =>
      ArrayBuffer.isView(value) && !(value instanceof DataView),
    isUint8Array: (value) => value instanceof Uint8Array,
    isUint8ClampedArray: (value) => value instanceof Uint8ClampedArray,
    isInt8Array: (value) => value instanceof Int8Array,
    isUint16Array: (value) => value instanceof Uint16Array,
    isInt16Array: (value) => value instanceof Int16Array,
    isUint32Array: (value) => value instanceof Uint32Array,
    isInt32Array: (value) => value instanceof Int32Array,
    isFloat32Array: (value) => value instanceof Float32Array,
    isFloat64Array: (value) => value instanceof Float64Array,
    isFloat16Array: (value) =>
      typeof Float16Array !== "undefined" && value instanceof Float16Array,
    isBigInt64Array: (value) => value instanceof BigInt64Array,
    isBigUint64Array: (value) => value instanceof BigUint64Array,
    isProxy: (value) => __nodeProxySet.has(value),
    isExternal: (value) => value && value.__quench_external === true,
  },
};
globalThis.__nodeUtil.inspect.defaultOptions = { numericSeparator: false };
NodeBuffer.INSPECT_MAX_BYTES = 50;
globalThis.__nodeUtil.formatWithOptions = (options, ...args) => {
  const previous =
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator;
  if (options && options.numericSeparator !== undefined)
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator =
      options.numericSeparator;
  try {
    return globalThis.__nodeUtil.format(...args);
  } finally {
    globalThis.__nodeUtil.inspect.defaultOptions.numericSeparator = previous;
  }
};
globalThis.__nodeQuerystring = {
  escape: (value) => encodeURIComponent(String(value)),
  unescape: (value) => decodeURIComponent(String(value)),
  stringify: (object, sep = "&", eq = "=") =>
    Object.keys(object)
      .map((key) => {
        const value = object[key];
        return (Array.isArray(value) ? value : [value])
          .map(
            (item) =>
              encodeURIComponent(key) + eq + encodeURIComponent(String(item)),
          )
          .join(sep);
      })
      .join(sep),
  parse: (input, sep = "&", eq = "=") =>
    String(input)
      .split(sep)
      .filter(Boolean)
      .reduce((result, part) => {
        const index = part.indexOf(eq);
        const key = decodeURIComponent(index < 0 ? part : part.slice(0, index));
        const value = decodeURIComponent(
          index < 0 ? "" : part.slice(index + eq.length),
        );
        result[key] =
          result[key] === undefined
            ? value
            : Array.isArray(result[key])
              ? result[key].concat(value)
              : [result[key], value];
        return result;
      }, {}),
};
class NodeURLSearchParams {
  constructor(init = "") {
    this._pairs = [];
    if (typeof init === "string") {
      init
        .replace(/^\?/, "")
        .split("&")
        .filter(Boolean)
        .forEach((part) => {
          const i = part.indexOf("=");
          this.append(
            decodeURIComponent(i < 0 ? part : part.slice(0, i)),
            decodeURIComponent(i < 0 ? "" : part.slice(i + 1)),
          );
        });
    } else Object.keys(init).forEach((key) => this.append(key, init[key]));
  }
  append(key, value) {
    this._pairs.push([String(key), String(value)]);
  }
  set(key, value) {
    this.delete(key);
    this.append(key, value);
  }
  get(key) {
    const pair = this._pairs.find(([name]) => name === String(key));
    return pair ? pair[1] : null;
  }
  getAll(key) {
    return this._pairs
      .filter(([name]) => name === String(key))
      .map(([, value]) => value);
  }
  has(key) {
    return this._pairs.some(([name]) => name === String(key));
  }
  delete(key) {
    this._pairs = this._pairs.filter(([name]) => name !== String(key));
  }
  toString() {
    return this._pairs
      .map(
        ([key, value]) =>
          `${encodeURIComponent(key)}=${encodeURIComponent(value)}`,
      )
      .join("&");
  }
}
globalThis.__nodeURLSearchParams = NodeURLSearchParams;
globalThis.__nodeURL = class NodeURL {
  constructor(input, base) {
    let value = String(input);
    if (base && !/^[a-z][a-z0-9+.-]*:/.test(value)) {
      const baseUrl = new NodeURL(base);
      value = value.startsWith("/")
        ? baseUrl.origin + value
        : baseUrl.origin + baseUrl.pathname.replace(/\/[^/]*$/, "/") + value;
    }
    const match = value.match(
      /^([a-z][a-z0-9+.-]*:)?(?:\/\/([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/i,
    );
    if (!match) throw new TypeError("Invalid URL");
    this.protocol = match[1] || "";
    this.host = match[2] || "";
    this.hostname = this.host.replace(/^.*@/, "").split(":")[0];
    this.port = this.host.includes(":")
      ? this.host.slice(this.host.lastIndexOf(":") + 1)
      : "";
    this.pathname = match[3] || "/";
    this.search = match[4] ? `?${match[4]}` : "";
    this.hash = match[5] ? `#${match[5]}` : "";
    this.origin =
      this.protocol && this.host ? `${this.protocol}//${this.host}` : "null";
    this.searchParams = new NodeURLSearchParams(match[4] || "");
  }
  get href() {
    const query = this.searchParams.toString();
    const prefix =
      this.protocol === "file:"
        ? "file://"
        : this.origin === "null"
          ? ""
          : this.origin;
    return `${prefix}${this.pathname}${query ? `?${query}` : this.search}${this.hash}`;
  }
  toString() {
    return this.href;
  }
};
globalThis.URL = globalThis.__nodeURL;
globalThis.URLSearchParams = globalThis.__nodeURLSearchParams;
globalThis.__nodeUrlModule = {
  URL: globalThis.__nodeURL,
  URLSearchParams: globalThis.__nodeURLSearchParams,
  fileURLToPath: (value) => {
    const href = String(value);
    if (!href.startsWith("file://"))
      throw new TypeError("URL must be a file URL");
    return decodeURIComponent(href.slice("file://".length)) || "/";
  },
  pathToFileURL: (value) =>
    new globalThis.__nodeURL(
      `file://${globalThis.__nodePath.resolve(String(value))}`,
    ),
  format: (value) =>
    value instanceof globalThis.__nodeURL ? value.href : String(value),
  resolve: (from, to) => new globalThis.__nodeURL(to, from).href,
};
globalThis.__nodeCrypto = {
  randomUUID: () => globalThis.__quench_random_uuid(),
  randomBytes: (size, callback) => {
    const output = NodeBuffer.allocUnsafe(Number(size));
    for (let i = 0; i < output.length; i++)
      output[i] = Math.floor(Math.random() * 256);
    if (typeof callback === "function")
      queueMicrotask(() => callback(null, output));
    return output;
  },
  randomFillSync: (buffer, offset = 0, size = buffer.length - offset) => {
    for (let i = offset; i < offset + size; i++)
      buffer[i] = Math.floor(Math.random() * 256);
    return buffer;
  },
  createHash: (algorithm) => {
    if (algorithm !== "sha256")
      throw new Error(`Unsupported hash: ${algorithm}`);
    let input = "";
    const hash = {
      update: (value) => {
        input += String(value);
        return hash;
      },
      digest: (encoding = "hex") => {
        const result = globalThis.__quench_sha256(input);
        if (encoding === "hex") return result;
        throw new Error(`Unsupported digest encoding: ${encoding}`);
      },
    };
    return hash;
  },
};
globalThis.require = (specifier) => {
  const name = String(specifier).replace(/^node:/, "");
  if (name === "assert") return globalThis.__nodeAssert;
  if (name === "path" || name === "path/posix") return globalThis.__nodePath;
  if (name === "util") return globalThis.__nodeUtil;
  if (name === "os") return globalThis.__nodeOs;
  if (name === "querystring") return globalThis.__nodeQuerystring;
  if (name === "url") return globalThis.__nodeUrlModule;
  if (name === "crypto") return globalThis.__nodeCrypto;
  if (name === "events")
    return {
      EventEmitter: globalThis.__nodeEventEmitter,
      once: globalThis.__nodeEventEmitter.once,
      on: globalThis.__nodeEventEmitter.on,
    };
  if (name === "stream") return globalThis.__nodeStream;
  if (name === "stream/iter")
    return {
      text: async (readable) => {
        const chunks = [];
        for await (const batch of readable)
          for (const chunk of batch) chunks.push(chunk);
        return new TextDecoder().decode(NodeBuffer.concat(chunks));
      },
      bytes: async (readable) => {
        const chunks = [];
        for await (const batch of readable)
          for (const chunk of batch) chunks.push(chunk);
        return NodeBuffer.concat(chunks);
      },
      pull: (readable, transform) => ({
        async *[Symbol.asyncIterator]() {
          for await (const batch of readable)
            yield transform ? transform(batch) : batch;
        },
      }),
    };
  if (name === "vm")
    return {
      runInThisContext: (code) => (0, eval)(String(code)),
      runInNewContext: (code, sandbox = {}) => {
        const previous = {};
        for (const key of Object.keys(sandbox)) {
          previous[key] = globalThis[key];
          globalThis[key] = sandbox[key];
        }
        try {
          return (0, eval)(String(code));
        } finally {
          for (const key of Object.keys(sandbox))
            globalThis[key] = previous[key];
        }
      },
      runInContext: (code, sandbox = {}) => {
        const previous = {};
        for (const key of Object.keys(sandbox)) {
          previous[key] = globalThis[key];
          globalThis[key] = sandbox[key];
        }
        try {
          return (0, eval)(String(code));
        } finally {
          for (const key of Object.keys(sandbox))
            globalThis[key] = previous[key];
        }
      },
    };
  if (name === "worker_threads") return { isMainThread: true };
  if (name === "internal/test/binding")
    return {
      internalBinding: (binding) =>
        binding === "uv"
          ? { UV_ENOENT: -2, UV_EEXIST: -17 }
          : binding === "js_stream"
            ? {
                JSStream: class JSStream {
                  constructor() {
                    this._externalStream = { __quench_external: true };
                  }
                },
              }
            : binding === "util"
              ? {
                  arrayBufferViewHasBuffer: (() => {
                    const observed = new WeakSet();
                    return (value) => {
                      if (value.byteLength >= 96 || observed.has(value))
                        return true;
                      observed.add(value);
                      return false;
                    };
                  })(),
                }
              : { fstat: () => undefined },
    };
  if (name === "internal/errors")
    return {
      codes: {
        ERR_OUT_OF_RANGE: class ERR_OUT_OF_RANGE extends RangeError {},
      },
    };
  if (name === "internal/buffer")
    return {
      utf8Write: (
        buffer,
        string,
        offset = 0,
        length = buffer.length - offset,
      ) => buffer.write(string, offset, length, "utf8"),
    };
  if (name === "internal/fs/utils")
    return {
      stringToFlags: (flags) => {
        const values = {
          r: 0,
          "r+": 2,
          rs: 1052674,
          "rs+": 1052674,
          sr: 1052674,
          "sr+": 1052674,
          w: 577,
          "w+": 578,
          wx: 705,
          xw: 705,
          "wx+": 706,
          "xw+": 706,
          a: 1089,
          "a+": 1090,
          ax: 1217,
          xa: 1217,
          "ax+": 1218,
          "xa+": 1218,
          as: 1051713,
          sa: 1051713,
          "as+": 1051714,
          "sa+": 1051714,
        };
        if (typeof flags !== "string" || values[flags] === undefined) {
          const error = new TypeError(`Unknown file open flag: ${flags}`);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        return values[flags];
      },
    };
  if (name === "zlib/iter")
    return {
      compressGzip: () => (chunks) => chunks,
      decompressGzip: () => (chunks) => chunks,
    };
  if (name === "timers") return globalThis.__nodeTimers;
  if (name === "timers/promises") return globalThis.__nodeTimersPromises;
  if (name === "../common" || name.endsWith("/common"))
    return globalThis.__nodeCommon;
  if (name.endsWith("/common/tmpdir")) return globalThis.__nodeTmpdir;
  if (name === "buffer") {
    const module = {
      Buffer: globalThis.Buffer,
      kMaxLength: 0x7fffffff,
      poolSize: NodeBuffer.poolSize,
      kStringMaxLength: 0x3fffffff,
      constants: {
        MAX_LENGTH: 0x7fffffff,
        MAX_STRING_LENGTH: 0x3fffffff,
      },
      isAscii: NodeBuffer.isAscii,
      isUtf8: NodeBuffer.isUtf8,
      atob: nodeAtob,
      btoa: nodeBtoa,
    };
    Object.defineProperty(module, "INSPECT_MAX_BYTES", {
      get: () => NodeBuffer.INSPECT_MAX_BYTES,
      set: (value) => {
        NodeBuffer.INSPECT_MAX_BYTES = value;
      },
    });
    return module;
  }
  if (name === "../common/fixtures" || name.endsWith("/common/fixtures"))
    return {
      fixturesDir: `${globalThis.__quench_cwd}/tests/node/test/fixtures`,
      path: (file) =>
        `${globalThis.__quench_cwd}/tests/node/test/fixtures/${file}`,
      utf8TestText: "The quick brown fox jumps over the lazy dog.\n",
    };
  if (name === "fs" || name === "fs/promises") return globalThis.__nodeFs;
  throw new Error(`Cannot find module '${specifier}'`);
};
