const __quenchOriginalRequireWithDecoder = globalThis.require;
const __quenchDecoderInputBytes = (input) => {
  if (ArrayBuffer.isView(input)) {
    return Array.from(
      new Uint8Array(input.buffer, input.byteOffset, input.byteLength),
    );
  }
  return Array.from(input || []);
};
const __quenchDecoderValidateInput = (decoder, input) => {
  if (!decoder || !Array.isArray(decoder._pending)) {
    const error = new TypeError("Cannot call write on an invalid receiver");
    error.code = "ERR_INVALID_THIS";
    throw error;
  }
  if (!ArrayBuffer.isView(input)) {
    const error = new TypeError(
      `The "buf" argument must be an instance of Buffer, TypedArray, or DataView.${
        __nodeInvalidArgSuffix(input)
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchDecoderStoreUtf8 = (decoder, bytes) => {
  const result = __quenchDecodeUtf8(bytes, false);
  decoder._pending = result.pending;
  decoder.lastNeed = result.pending.length ? 3 - result.pending.length : 0;
  decoder.lastTotal = result.pending.length ? 3 : 0;
  decoder.lastChar.fill(0);
  decoder.lastChar.set(result.pending);
  return result.text;
};
const __quenchStringDecoderClass = class {
  constructor(encoding = "utf8") {
    this.encoding = String(encoding).toLowerCase().replace("-", "");
    if (
      !["utf8", "ucs2", "utf16le", "latin1", "ascii"].includes(this.encoding)
    ) {
      const error = new TypeError(`Unknown encoding: ${encoding}`);
      error.code = "ERR_UNKNOWN_ENCODING";
      throw error;
    }
    this._decoder = new TextDecoder(
      this.encoding === "utf8" ? "utf-8" : this.encoding,
    );
    this._pending = [];
    this.lastNeed = 0;
    this.lastTotal = 0;
    this.lastChar = new NodeBuffer(4);
  }
  write(input) {
    __quenchDecoderValidateInput(this, input);
    const bytes = [...this._pending, ...__quenchDecoderInputBytes(input)];
    if (this.encoding === "utf8") {
      return __quenchDecoderStoreUtf8(this, bytes);
    }
    if (this.encoding === "ucs2" || this.encoding === "utf16le") {
      const result = __quenchDecodeUtf16(bytes, false);
      this._pending = result.pending;
      return result.text;
    }
    let end = 0;
    while (end < bytes.length) {
      const width = bytes[end] < 0x80
        ? 1
        : bytes[end] < 0xe0
        ? 2
        : bytes[end] < 0xf0
        ? 3
        : 4;
      if (end + width > bytes.length) break;
      end += width;
    }
    this._pending = bytes.slice(end);
    return this._decoder.decode(new Uint8Array(bytes.slice(0, end)));
  }
  text(input, offset = 0) {
    return offset >= input.length ? "" : this.write(input.slice(offset));
  }
  end(input) {
    if (this.encoding === "utf8") {
      const prefix = input === undefined ? "" : this.write(input);
      const result = __quenchDecodeUtf8(this._pending, true);
      this._pending = [];
      return `${prefix}${result.text}`;
    }
    if (this.encoding === "ucs2" || this.encoding === "utf16le") {
      const prefix = input === undefined ? "" : this.write(input);
      return `${prefix}${__quenchDecodeUtf16(this._pending, true).text}`;
    }
    return `${input === undefined ? "" : this.write(input)}${
      this._decoder.decode(new Uint8Array(this._pending))
    }`;
  }
};
const __quenchStringDecoder = function __quenchStringDecoder(encoding) {
  const state = new __quenchStringDecoderClass(encoding);
  Object.setPrototypeOf(this, __quenchStringDecoderClass.prototype);
  Object.assign(this, state);
};
__quenchStringDecoder.prototype = __quenchStringDecoderClass.prototype;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "string_decoder") {
    return { StringDecoder: __quenchStringDecoder };
  }
  return __quenchOriginalRequireWithDecoder(specifier);
};
