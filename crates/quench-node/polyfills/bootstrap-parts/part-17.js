const __quenchOriginalRequireWithDecoder = globalThis.require;
class __quenchStringDecoder {
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
    return `${input === undefined ? "" : this.write(input)}${this._decoder.decode(new Uint8Array(this._pending))}`;
  }
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "string_decoder")
    return { StringDecoder: __quenchStringDecoder };
  return __quenchOriginalRequireWithDecoder(specifier);
};
