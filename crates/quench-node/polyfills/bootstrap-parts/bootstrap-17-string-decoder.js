const __quenchOriginalRequireWithDecoder = globalThis.require;
const __quenchStringDecoderClass = class {
  constructor(encoding = "utf8") {
    this.encoding = String(encoding).toLowerCase().replace("-", "");
    this._decoder = new TextDecoder(
      this.encoding === "utf8" ? "utf-8" : this.encoding
    );
    this._pending = [];
    this.lastNeed = 0;
    this.lastTotal = 0;
    this.lastChar = new Uint8Array(4);
  }
  write(input) {
    const bytes = [...this._pending, ...Array.from(input || [])];
    if (this.encoding === "utf8") {
      const result = __quenchDecodeUtf8(bytes, false);
      this._pending = result.pending;
      return result.text;
    }
    let end = 0;
    while (end < bytes.length) {
      const width =
        bytes[end] < 0x80
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
  end(input) {
    if (this.encoding === "utf8") {
      const prefix = input === undefined ? "" : this.write(input);
      return `${prefix}${__quenchDecodeUtf8(this._pending, true).text}`;
    }
    return `${input === undefined ? "" : this.write(input)}${this._decoder.decode(new Uint8Array(this._pending))}`;
  }
};
function __quenchStringDecoder(encoding) {
  const state = new __quenchStringDecoderClass(encoding);
  Object.setPrototypeOf(this, __quenchStringDecoderClass.prototype);
  Object.assign(this, state);
}
__quenchStringDecoder.prototype = __quenchStringDecoderClass.prototype;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "string_decoder")
    return { StringDecoder: __quenchStringDecoder };
  return __quenchOriginalRequireWithDecoder(specifier);
};
