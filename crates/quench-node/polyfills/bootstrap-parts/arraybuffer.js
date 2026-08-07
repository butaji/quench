const __nodeBufferIsArrayBuffer = (value) => {
  try {
    Object.getOwnPropertyDescriptor(
      ArrayBuffer.prototype,
      "byteLength",
    ).get.call(value);
    return true;
  } catch {
    return false;
  }
};
