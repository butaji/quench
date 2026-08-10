//! Polyfill: `pool`

pub const JS: &str = r#"const __NodeBufferBase01 = NodeBuffer;
const __nodeBufferPoolSize = 8192;
let __nodeBufferPool = new ArrayBuffer(__nodeBufferPoolSize);
let __nodeBufferPoolOffset = 0;
const __nodeUntransferableBuffers = new WeakSet([__nodeBufferPool]);
const __nodeArrayBufferTransfer = ArrayBuffer.prototype.transfer;
ArrayBuffer.prototype.transfer = function (...args) {
  if (__nodeUntransferableBuffers.has(this)) {
    throw new TypeError("Cannot transfer an untransferable ArrayBuffer");
  }
  if (typeof __nodeArrayBufferTransfer === "function") {
    return __nodeArrayBufferTransfer.apply(this, args);
  }
  if (!(this instanceof ArrayBuffer)) {
    throw new TypeError(
      "Method ArrayBuffer.prototype.transfer called on incompatible receiver"
    );
  }
  const maxByteLength = args[0];
  if (
    maxByteLength !== undefined &&
    (!Number.isSafeInteger(maxByteLength) || maxByteLength < this.byteLength)
  ) {
    throw new RangeError("Invalid array buffer length");
  }
  const result = new ArrayBuffer(maxByteLength ?? this.byteLength);
  new Uint8Array(result).set(new Uint8Array(this));
  return result;
};
const __nodeBufferPoolFrom = (source) => {
  if (source.length === 0 || source.length >= __nodeBufferPoolSize >>> 1) {
    return undefined;
  }
  if (__nodeBufferPoolOffset + source.length > __nodeBufferPool.byteLength) {
    __nodeBufferPool = new ArrayBuffer(__nodeBufferPoolSize);
    __nodeUntransferableBuffers.add(__nodeBufferPool);
    __nodeBufferPoolOffset = 0;
  }
  const result = new __NodeBufferBase01(
    __nodeBufferPool,
    __nodeBufferPoolOffset,
    source.length
  );
  result.set(source);
  __nodeBufferPoolOffset += source.length;
  return result;
};
"#;
