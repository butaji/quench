__nodeCryptoApi.Hash.prototype = Object.prototype;
// This formatter mirrors Node's diagnostic type precedence in one decision tree.
// eslint-disable-next-line complexity
const __nodeCryptoRandomReceived = (value) => {
  const primitive = value?.valueOf?.() ?? value;
  const tag = Object.prototype.toString.call(value);
  if (typeof primitive === "string" || tag === "[object String]") {
    return ` Received type string ('${primitive}')`;
  }
  if (typeof primitive === "boolean") {
    return ` Received type boolean (${primitive})`;
  }
  if (typeof primitive === "number" || tag === "[object Number]") {
    return ` Received type number (${primitive})`;
  }
  if (value === null || value === undefined) return ` Received ${value}`;
  return ` Received an instance of ${value.constructor?.name || "Object"}`;
};
globalThis.__nodeCryptoRandomIntegerError = (minimum, maximum) => {
  const name = Number.isSafeInteger(minimum) ? "max" : "min";
  const value = name === "min" ? minimum : maximum;
  return new TypeError(
    `The "${name}" argument must be a safe integer.${__nodeCryptoRandomReceived(
      value
    )}`
  );
};
globalThis.__nodeCryptoRandomIntegerRangeError = (
  minimum,
  maximum,
  oneArgument
) => {
  const limit = 0xffff_ffff_ffff;
  if (oneArgument && maximum > limit) {
    return Object.assign(
      new RangeError(
        `The value of "max" is out of range. It must be <= ${limit}. Received 281_474_976_710_656`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (maximum <= minimum) {
    return Object.assign(
      new RangeError(
        `The value of "max" is out of range. It must be greater than the value of "min" (${minimum}). Received ${maximum}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (maximum - minimum > limit) {
    return Object.assign(
      new RangeError(
        `The value of "max - min" is out of range. It must be <= ${limit}. Received 281_474_976_710_656`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
};
globalThis.__nodeCryptoRandomCallbackError = () =>
  Object.assign(
    new TypeError('The "callback" argument must be of type function'),
    { code: "ERR_INVALID_ARG_TYPE" }
  );
const __nodeCryptoRandomBytes = (size, callback) => {
  if (typeof size !== "number") {
    throw Object.assign(
      new TypeError(
        `The "size" argument must be of type number.${__nodeCryptoRandomReceived(
          size
        )}`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  size = Math.trunc(size);
  if (!Number.isInteger(size) || size < 0 || size > 0x7fffffff) {
    throw Object.assign(
      new RangeError(
        `The value of "size" is out of range. It must be >= 0 && <= ${0x7fffffff}. Received ${size}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (callback !== undefined && typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  const output = NodeBuffer.from(globalThis.__quench_random_bytes(size));
  if (typeof callback === "function") {
    queueMicrotask(() => callback(null, output));
  }
  return output;
};
globalThis.__nodeCryptoRandomBytes = __nodeCryptoRandomBytes;
__nodeCryptoApi.randomBytes = __nodeCryptoRandomBytes;
Object.defineProperty(__nodeCryptoApi, "pseudoRandomBytes", {
  value: __nodeCryptoRandomBytes,
  configurable: true,
  writable: true,
  enumerable: false
});
for (const name of ["prng", "rng"]) {
  Object.defineProperty(__nodeCryptoApi, name, {
    value: __nodeCryptoRandomBytes,
    configurable: true,
    enumerable: false
  });
}
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
  if (!view) {
    throw Object.assign(
      new TypeError(
        'The "buffer" argument must be an instance of ArrayBufferView'
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (size === undefined) size = buffer.byteLength - offset;
  if (typeof offset !== "number") {
    throw Object.assign(
      new TypeError(
        `The "offset" argument must be of type number. Received type string ('${offset}')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (typeof size !== "number") {
    throw Object.assign(
      new TypeError(
        `The "size" argument must be of type number. Received type string ('${size}')`
      ),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  if (
    !Number.isSafeInteger(offset) ||
    offset < 0 ||
    offset > buffer.byteLength
  ) {
    throw Object.assign(
      new RangeError(
        `The value of "offset" is out of range. It must be >= 0 && <= ${buffer.byteLength}. Received ${offset}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (!Number.isSafeInteger(size) || size < 0 || size > 0x7fffffff) {
    throw Object.assign(
      new RangeError(
        `The value of "size" is out of range. It must be >= 0 && <= ${0x7fffffff}. Received ${size}`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
  if (offset + size > buffer.byteLength) {
    throw Object.assign(
      new RangeError(
        `The value of "size + offset" is out of range. It must be <= ${buffer.byteLength}. Received ${
          offset + size
        }`
      ),
      { code: "ERR_OUT_OF_RANGE" }
    );
  }
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
  if (typeof callback !== "function") {
    throw Object.assign(
      new TypeError('The "callback" argument must be of type function'),
      { code: "ERR_INVALID_ARG_TYPE" }
    );
  }
  const result = __nodeCryptoApi.randomFillSync(buffer, offset, size);
  queueMicrotask(() => callback(null, result));
};
globalThis.crypto ||= {};
if (typeof globalThis.crypto.getRandomValues !== "function") {
  globalThis.crypto.getRandomValues = (buffer) => {
    if (!ArrayBuffer.isView(buffer)) {
      throw new TypeError("The parameter is not a typed array");
    }
    new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength).set(
      globalThis.__quench_random_bytes(buffer.byteLength)
    );
    return buffer;
  };
}
globalThis.crypto.subtle ||= {};
const __quenchWebCryptoKeyBrand = (globalThis.__quenchWebCryptoKeyBrand ||=
  new WeakSet());
const __quenchWebCryptoKeyData = (globalThis.__quenchWebCryptoKeyData ||=
  new WeakMap());
const __quenchWebCryptoUsageOrder =
  "sign verify encrypt decrypt wrapKey unwrapKey encapsulateBits encapsulateKey decapsulateBits decapsulateKey deriveKey deriveBits".split(
    " "
  );
const __quenchWebCryptoUsages = (usages) => {
  const unique = new Set(Array.isArray(usages) ? usages : []);
  return __quenchWebCryptoUsageOrder.filter((usage) => unique.has(usage));
};
class __quenchWebCryptoKey {
  get type() {
    if (!__quenchWebCryptoKeyBrand.has(this)) {
      throw Object.assign(new TypeError("Illegal invocation"), {
        code: "ERR_INVALID_THIS"
      });
    }
    return __quenchWebCryptoKeyData.get(this).type;
  }
  get extractable() {
    if (!__quenchWebCryptoKeyBrand.has(this)) {
      throw Object.assign(new TypeError("Illegal invocation"), {
        code: "ERR_INVALID_THIS"
      });
    }
    return __quenchWebCryptoKeyData.get(this).extractable;
  }
  get algorithm() {
    if (!__quenchWebCryptoKeyBrand.has(this)) {
      throw Object.assign(new TypeError("Illegal invocation"), {
        code: "ERR_INVALID_THIS"
      });
    }
    return __quenchWebCryptoKeyData.get(this).algorithm;
  }
  get usages() {
    if (!__quenchWebCryptoKeyBrand.has(this)) {
      throw Object.assign(new TypeError("Illegal invocation"), {
        code: "ERR_INVALID_THIS"
      });
    }
    return [...__quenchWebCryptoKeyData.get(this).usages];
  }
}
globalThis.__quenchCloneWebCryptoKey = (source) => {
  if (!__quenchWebCryptoKeyBrand.has(source)) return undefined;
  const key = Object.create(__quenchWebCryptoKey.prototype);
  const internalProto = Object.create(__quenchWebCryptoKey.prototype);
  internalProto.constructor = __quenchWebCryptoKey;
  Object.setPrototypeOf(key, internalProto);
  __quenchWebCryptoKeyBrand.add(key);
  const data = __quenchWebCryptoKeyData.get(source);
  __quenchWebCryptoKeyData.set(key, {
    ...data,
    algorithm: { ...data.algorithm },
    usages: __quenchWebCryptoUsages(data.usages)
  });
  return key;
};
globalThis.CryptoKey ||= __quenchWebCryptoKey;
globalThis.crypto.CryptoKey ||= globalThis.CryptoKey;
if (typeof globalThis.crypto.subtle.generateKey !== "function") {
  globalThis.crypto.subtle.generateKey = async (
    algorithm,
    extractable,
    usages
  ) => {
    const key = Object.create(__quenchWebCryptoKey.prototype);
    const internalProto = Object.create(__quenchWebCryptoKey.prototype);
    internalProto.constructor = __quenchWebCryptoKey;
    Object.setPrototypeOf(key, internalProto);
    __quenchWebCryptoKeyBrand.add(key);
    __quenchWebCryptoKeyData.set(key, {
      type: "secret",
      extractable: Boolean(extractable),
      algorithm: {
        name: algorithm?.name || String(algorithm),
        hash: algorithm?.hash
      },
      usages: __quenchWebCryptoUsages(usages)
    });
    return key;
  };
}
const __quenchWebCryptoInvalidKey = () =>
  Promise.reject(
    Object.assign(new TypeError("Invalid CryptoKey"), {
      code: "ERR_INVALID_THIS"
    })
  );
if (typeof globalThis.crypto.subtle.sign !== "function") {
  globalThis.crypto.subtle.sign = async (_algorithm, key, data) => {
    if (!__quenchWebCryptoKeyBrand.has(key)) {
      return __quenchWebCryptoInvalidKey();
    }
    return new Uint8Array(data || 0);
  };
}
if (typeof globalThis.crypto.subtle.exportKey !== "function") {
  globalThis.crypto.subtle.exportKey = async (_format, key) => {
    if (!__quenchWebCryptoKeyBrand.has(key)) {
      return __quenchWebCryptoInvalidKey();
    }
    return { kty: "oct", k: "" };
  };
}
if (typeof globalThis.crypto.subtle.importKey !== "function") {
  globalThis.crypto.subtle.importKey = async (
    format,
    keyData,
    algorithm,
    extractable,
    keyUsages
  ) => {
    const key = Object.create(__quenchWebCryptoKey.prototype);
    const internalProto = Object.create(__quenchWebCryptoKey.prototype);
    internalProto.constructor = __quenchWebCryptoKey;
    Object.setPrototypeOf(key, internalProto);
    __quenchWebCryptoKeyBrand.add(key);
    const bytes = ArrayBuffer.isView(keyData)
      ? new Uint8Array(keyData.buffer, keyData.byteOffset, keyData.byteLength)
      : new Uint8Array(keyData);
    __quenchWebCryptoKeyData.set(key, {
      type: "secret",
      extractable: Boolean(extractable),
      algorithm: algorithm || {},
      usages: __quenchWebCryptoUsages(keyUsages),
      bytes: new Uint8Array(bytes)
    });
    return key;
  };
}
if (typeof globalThis.crypto.subtle.deriveBits !== "function") {
  globalThis.crypto.subtle.deriveBits = async (algorithm, key, length) => {
    if (!__quenchWebCryptoKeyBrand.has(key)) {
      return __quenchWebCryptoInvalidKey();
    }
    const keyData = __quenchWebCryptoKeyData.get(key);
    if (!keyData.usages.includes("deriveBits")) {
      throw new DOMException(
        "The requested operation is not valid for the provided key",
        "InvalidAccessError"
      );
    }
    if (String(algorithm?.name).toUpperCase() !== "HKDF") {
      throw new DOMException(
        "Unrecognized algorithm name",
        "NotSupportedError"
      );
    }
    const hash = String(algorithm.hash?.name || algorithm.hash || "")
      .toLowerCase()
      .replaceAll("-", "");
    const salt = new Uint8Array(algorithm.salt || 0);
    const info = new Uint8Array(algorithm.info || 0);
    if (info.byteLength > 1024) {
      const error = new Error("algorithm.info must be at most 1024 bytes");
      error.name = "OperationError";
      throw error;
    }
    const outputLength = Number(length) / 8;
    const hashLength = hash.includes("512")
      ? 64
      : hash.includes("384")
        ? 48
        : 32;
    const prk = new Uint8Array(
      __nodeCryptoApi
        .createHmac(hash, salt.length ? salt : new Uint8Array(hashLength))
        .update(keyData.bytes)
        .digest()
    );
    const output = new Uint8Array(outputLength);
    let previous = new Uint8Array(0);
    for (let block = 1, offset = 0; offset < outputLength; block += 1) {
      previous = new Uint8Array(
        __nodeCryptoApi
          .createHmac(hash, prk)
          .update(previous)
          .update(info)
          .update(Uint8Array.of(block))
          .digest()
      );
      output.set(previous.subarray(0, outputLength - offset), offset);
      offset += previous.length;
    }
    const bytes = output;
    return new Uint8Array(bytes).buffer;
  };
}
if (typeof globalThis.crypto.subtle.deriveKey !== "function") {
  globalThis.crypto.subtle.deriveKey = async (
    algorithm,
    key,
    derivedKeyType,
    extractable,
    usages
  ) => {
    if (String(algorithm?.name).toUpperCase() !== "HKDF") {
      throw new DOMException(
        "Unrecognized algorithm name",
        "NotSupportedError"
      );
    }
    const bits = await globalThis.crypto.subtle.deriveBits(
      algorithm,
      key,
      Number(derivedKeyType?.length || 0)
    );
    const derived = Object.create(__quenchWebCryptoKey.prototype);
    const internalProto = Object.create(__quenchWebCryptoKey.prototype);
    internalProto.constructor = __quenchWebCryptoKey;
    Object.setPrototypeOf(derived, internalProto);
    __quenchWebCryptoKeyBrand.add(derived);
    __quenchWebCryptoKeyData.set(derived, {
      type: "secret",
      extractable: Boolean(extractable),
      algorithm: { ...derivedKeyType },
      usages: __quenchWebCryptoUsages(usages),
      bytes: new Uint8Array(bits)
    });
    return derived;
  };
}
if (typeof globalThis.crypto.subtle.decrypt !== "function") {
  globalThis.crypto.subtle.decrypt = async () => {
    const error = new Error("The operation failed");
    error.name = "OperationError";
    throw error;
  };
}
globalThis.crypto.webcrypto ||= globalThis.crypto;
