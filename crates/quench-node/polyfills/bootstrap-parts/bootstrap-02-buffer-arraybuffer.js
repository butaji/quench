const __nodeBufferIsArrayBuffer = (value) => {
  if (!(value instanceof ArrayBuffer)) return false;
  try {
    Object.getOwnPropertyDescriptor(
      ArrayBuffer.prototype,
      "byteLength"
    ).get.call(value);
    return true;
  } catch {
    return false;
  }
};
