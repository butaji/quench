const __nodeCryptoRandomReceived = (value) => {
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (value === null || value === undefined) return ` Received ${value}`;
  if (typeof value === "boolean") return ` Received type boolean (${value})`;
  return ` Received an instance of ${value.constructor?.name || "Object"}`;
};
const __nodeCryptoRandomBytes = (size, callback) => {
  if (typeof size !== "number")
    throw Object.assign(
      new TypeError(
        `The "size" argument must be of type number.${__nodeCryptoRandomReceived(size)}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  size = Math.floor(size);
  if (!Number.isFinite(size) || size < 0 || size > 0x7fffffff)
    throw Object.assign(
      new RangeError(
        `The value of "size" is out of range. It must be >= 0 && <= ${0x7fffffff}. Received ${size}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  if (callback !== undefined && typeof callback !== "function")
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  const output = NodeBuffer.from(globalThis.__quench_random_bytes(size));
  if (typeof callback === "function")
    queueMicrotask(() => callback(null, output));
  return output;
};
__nodeCryptoApi.randomBytes = __nodeCryptoRandomBytes;
Object.defineProperty(__nodeCryptoApi, "pseudoRandomBytes", {
  value: __nodeCryptoRandomBytes,
  configurable: true,
  writable: true,
  enumerable: false
});
for (const name of ["prng", "rng"])
  Object.defineProperty(__nodeCryptoApi, name, {
    value: __nodeCryptoRandomBytes,
    configurable: true,
    enumerable: false
  });
globalThis.__nodeCryptoApi = __nodeCryptoApi;
globalThis.__nodeCryptoApi.pseudoRandomBytes = __nodeCryptoRandomBytes;
// eslint-disable-next-line max-lines-per-function -- shared validation and byte-view handling
__nodeCryptoApi.randomFillSync = (
  buffer,
  offset = 0,
  size
  // eslint-disable-next-line complexity -- shared validation and byte-view handling
) => {
  const view = ArrayBuffer.isView(buffer)
    ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
    : buffer instanceof ArrayBuffer ||
        (typeof SharedArrayBuffer !== "undefined" &&
          buffer instanceof SharedArrayBuffer)
      ? new Uint8Array(buffer)
      : null;
  if (!view)
    throw Object.assign(
      new TypeError(
        'The "buffer" argument must be an instance of ArrayBufferView'
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (size === undefined) size = buffer.byteLength - offset;
  if (typeof offset !== "number")
    throw Object.assign(
      new TypeError(
        `The "offset" argument must be of type number. Received type string ('${offset}')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (typeof size !== "number")
    throw Object.assign(
      new TypeError(
        `The "size" argument must be of type number. Received type string ('${size}')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > buffer.byteLength)
    throw Object.assign(
      new RangeError(
        `The value of "offset" is out of range. It must be >= 0 && <= ${buffer.byteLength}. Received ${offset}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  if (!Number.isSafeInteger(size) || size < 0 || size > 0x7fffffff)
    throw Object.assign(
      new RangeError(
        `The value of "size" is out of range. It must be >= 0 && <= ${0x7fffffff}. Received ${size}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  if (offset + size > buffer.byteLength)
    throw Object.assign(
      new RangeError(
        `The value of "size + offset" is out of range. It must be <= ${buffer.byteLength}. Received ${offset + size}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  view.set(globalThis.__quench_random_bytes(size), offset);
  return buffer;
};
__nodeCryptoApi.randomFill = (buffer, offset, size, callback) => {
  if (typeof offset === "function") {
    callback = offset;
    offset = 0;
    size = buffer?.byteLength;
  } else if (typeof size === "function") {
    callback = size;
    size = buffer?.byteLength - offset;
  }
  if (typeof callback !== "function")
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  const result = __nodeCryptoApi.randomFillSync(buffer, offset, size);
  queueMicrotask(() => callback(null, result));
};
globalThis.crypto ||= {};
if (typeof globalThis.crypto.getRandomValues !== "function")
  globalThis.crypto.getRandomValues = (buffer) => {
    if (!ArrayBuffer.isView(buffer))
      throw new TypeError("The parameter is not a typed array");
    new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength).set(
      globalThis.__quench_random_bytes(buffer.byteLength)
    );
    return buffer;
  };
