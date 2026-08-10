//! Polyfill: `api-head`

pub const JS: &str = r#"globalThis.Buffer = new Proxy(NodeBuffer, {
  apply(_target, _thisArg, args) {
    if (typeof args[0] === "number") {
      return new NodeBuffer(NodeBuffer._validateSize(args[0]));
    }
    return NodeBuffer.from(...args);
  },
  construct(_target, args) {
    if (typeof args[0] === "number") {
      if (typeof args[1] === "string") {
        const error = new TypeError(
          `The "string" argument must be of type string. Received type number (${
            args[0]
          })`
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const buffer = new NodeBuffer(NodeBuffer._validateSize(args[0]));
      Object.defineProperties(buffer, {
        parent: { value: buffer.buffer, configurable: true },
        offset: { value: buffer.byteOffset, configurable: true }
      });
      return buffer;
    }
    return NodeBuffer.from(...args);
  }
});
Object.defineProperties(NodeBuffer.prototype, {
  parent: { value: undefined, configurable: true },
  offset: { value: undefined, configurable: true }
});
NodeBuffer.poolSize = 8192;
for (const name of "ascii base64 base64url latin1 hex ucs2 utf8".split(" ")) {
  NodeBuffer.prototype[`${name}Slice`] = NodeBuffer.prototype.slice;
  NodeBuffer.prototype[`${name}Write`] = NodeBuffer.prototype.write;
}
NodeBuffer.prototype[Symbol.for("nodejs.util.inspect.custom")] =
  NodeBuffer.prototype.inspect;
for (const name of ["8", "16LE", "16BE", "32LE", "32BE"]) {
  NodeBuffer.prototype[`readUint${name}`] =
    NodeBuffer.prototype[`readUInt${name}`];
  NodeBuffer.prototype[`writeUint${name}`] =
    NodeBuffer.prototype[`writeUInt${name}`];
}
NodeBuffer.prototype.readUintLE = NodeBuffer.prototype.readUIntLE;
NodeBuffer.prototype.toLocaleString = NodeBuffer.prototype.toString;
NodeBuffer.prototype.readUintBE = NodeBuffer.prototype.readUIntBE;
NodeBuffer.prototype.writeUintLE = NodeBuffer.prototype.writeUIntLE;
NodeBuffer.prototype.writeUintBE = NodeBuffer.prototype.writeUIntBE;
NodeBuffer.prototype.readBigUint64LE = NodeBuffer.prototype.readBigUInt64LE;
NodeBuffer.prototype.readBigUint64BE = NodeBuffer.prototype.readBigUInt64BE;
NodeBuffer.prototype.writeBigUint64LE = NodeBuffer.prototype.writeBigUInt64LE;
NodeBuffer.prototype.writeBigUint64BE = NodeBuffer.prototype.writeBigUInt64BE;
const __nodeGetOwnPropertyNames = Object.getOwnPropertyNames;
Object.getOwnPropertyNames = (value) => {
  if (value !== NodeBuffer.prototype) return __nodeGetOwnPropertyNames(value);
  const names = new Set();
  for (
    let prototype = value;
    prototype && prototype !== Uint8Array.prototype;
    prototype = Object.getPrototypeOf(prototype)
  ) {
    for (const name of __nodeGetOwnPropertyNames(prototype)) {
      if (
        !name.startsWith("_") &&
        typeof Object.getOwnPropertyDescriptor(prototype, name)?.value ===
          "function"
      ) {
        names.add(name);
      }
    }
  }
  return Array.from(names);
};
const __nodeInvalidCharacter = () => {
  const error = new DOMException(
    "The string contains invalid characters.",
    "InvalidCharacterError"
  );
  error.code = 5;
  return error;
};
function nodeAtob(value) {
  if (arguments.length === 0 || typeof value === "symbol") {
    throw new TypeError("The data is not a string");
  }
  const input = String(value).replace(/[\t\n\f\r ]/g, "");
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(input) || input.length % 4 === 1) {
    throw __nodeInvalidCharacter();
  }
  return NodeBuffer.from(input, "base64").toString("latin1");
}
function nodeBtoa(value) {
  if (arguments.length === 0 || typeof value === "symbol") {
    throw new TypeError("The data is not a string");
  }
  const input = String(value);
  for (let index = 0; index < input.length; index++) {
    if (input.charCodeAt(index) > 255) throw __nodeInvalidCharacter();
  }
  return NodeBuffer.from(input, "latin1").toString("base64");
}
const __nodeEncodeCodePoint = (output, code) => {
  if (code < 0x80) return output.push(code);
  if (code < 0x800) {
    return output.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
  }
  if (code < 0x10000) {
    return output.push(
      0xe0 | (code >> 12),
      0x80 | ((code >> 6) & 0x3f),
      0x80 | (code & 0x3f)
    );
  }
  return output.push(
    0xf0 | (code >> 18),
    0x80 | ((code >> 12) & 0x3f),
    0x80 | ((code >> 6) & 0x3f),
    0x80 | (code & 0x3f)
  );
};
const __nodeReadCodePoint = (input, index) => {
  const code = input.charCodeAt(index);
  if (code < 0xd800 || code > 0xdfff) return [code, index];
  const next = input.charCodeAt(index + 1);
  if (code <= 0xdbff && next >= 0xdc00 && next <= 0xdfff) {
    return [0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00), index + 1];
  }
  return [0xfffd, index];
};
class NodeTextEncoder {
  encode(value) {
    const output = [];
    const input = String(value);
    let ascii = true;
    for (let index = 0; index < input.length; index++) {
      if (input.charCodeAt(index) > 0x7f) {
        ascii = false;
        break;
      }
    }
    if (ascii) {
      const encoded = new Uint8Array(input.length);
      for (let index = 0; index < input.length; index++) {
        encoded[index] = input.charCodeAt(index);
      }
      return encoded;
    }
    for (let index = 0; index < input.length; index++) {
      const [code, nextIndex] = __nodeReadCodePoint(input, index);
      index = nextIndex;
      __nodeEncodeCodePoint(output, code);
    }
    return new Uint8Array(output);
  }
}
globalThis.TextEncoder = NodeTextEncoder;
const __nodeWindows1252 = {
  128: "€",
  130: "‚",
  131: "ƒ",
  132: "„",
  133: "…",
  134: "†",
  135: "‡",
  136: "ˆ",
  137: "‰",
  138: "Š",
  139: "‹",
  140: "Œ",
  142: "Ž",
  145: "‘",
  146: "’",
  147: "“",
  148: "”",
  149: "•",
  150: "–",
  151: "—",
  152: "˜",
  153: "™",
  154: "š",
  155: "›",
  156: "œ",
  158: "ž",
  159: "Ÿ"
};
class NodeTextDecoder {
  constructor(encoding = "utf-8") {
    this.encoding = String(encoding).toLowerCase();
  }
  decode(bytes) {
    let result = "";
    for (let i = 0; i < bytes.length;) {
      const first = bytes[i++];
      if (this.encoding === "windows-1252" && first >= 128) {
        result += __nodeWindows1252[first] || String.fromCodePoint(first);
      } else if (first < 0x80) result += String.fromCodePoint(first);
      else if (first < 0xe0) {
        result += String.fromCodePoint(
          ((first & 0x1f) << 6) | (bytes[i++] & 0x3f)
        );
      } else if (first < 0xf0) {
        result += String.fromCodePoint(
          ((first & 0x0f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f)
        );
      } else {
        result += String.fromCodePoint(
          ((first & 7) << 18) |
            ((bytes[i++] & 0x3f) << 12) |
            ((bytes[i++] & 0x3f) << 6) |
            (bytes[i++] & 0x3f)
        );
      }
    }
    return result;
  }
}
globalThis.TextDecoder = NodeTextDecoder;
const nodePathFromURL = (value) => {
  if (value.protocol !== "file:") {
    throw Object.assign(new TypeError("The URL must use the file: protocol"), { code: "ERR_INVALID_URL_SCHEME" });
  }
  return globalThis.__nodeUrlModule.fileURLToPath(value);
};
const nodePathValue = (value) =>
  value instanceof NodeBuffer
    ? value.toString()
    : value instanceof Uint8Array
      ? new NodeTextDecoder().decode(value)
      : value instanceof globalThis.__nodeURL
        ? nodePathFromURL(value)
        : String(value);
const nodeFsPath = (value) => {
  if (
    typeof value === "string" ||
    value instanceof NodeBuffer ||
    value instanceof Uint8Array ||
    value instanceof globalThis.__nodeURL
  ) {
    return nodePathValue(value);
  }
  const error = new TypeError(
    'The "path" argument must be of type string or an instance of Buffer or URL.'
  );
  error.message += globalThis.__nodeCommon.invalidArgTypeHelper(value);
  error.code = "ERR_INVALID_ARG_TYPE";
  throw error;
};
"#;
