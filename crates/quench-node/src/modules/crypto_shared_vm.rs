//! The hash surface used by the pinned framework package paths.
//!
//! `etag` calls `createHash("sha1").update(string, "utf8").digest("base64")`.
//! Keep that Node-facing wrapper in the guest realm and the digest primitive
//! in Rust; it shares the SHA-1 implementation with the legacy Node module.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const CRYPTO_FACTORY: &str = quench_js_check::checked_js!(
r#"(hashDigest, hmacDigest, signDigest, verifyDigest, Buffer, randomBytes, pbkdf2, scryptNative, Transform, cipherProcess) => {
  const states = new WeakMap();
  const secretKeys = new WeakMap();
  const kHandle = Symbol.for("quench.internal.crypto.kHandle");
  let repeatedHmacDigestWarningEmitted = false;
  const unsupportedDigest = (algorithm) => {
    const error = new Error(`Invalid digest: ${algorithm}`);
    error.code = "ERR_OSSL_EVP_UNSUPPORTED";
    return error;
  };
  const finalized = () => {
    const error = new Error("Digest already called");
    error.code = "ERR_CRYPTO_HASH_FINALIZED";
    return error;
  };
  const inputBuffer = (data, encoding) => {
    if (typeof data === "string") {
      const name = encoding === undefined ? "utf8" : String(encoding).toLowerCase();
      if (name === "hex" && data.length % 2 !== 0) {
        const error = new TypeError(`The argument 'encoding' is invalid for data of length ${data.length}. Received 'hex'`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      return Buffer.from(data, name);
    }
    if (data !== null && typeof data === "object" &&
        (ArrayBuffer.isView(data) || data instanceof ArrayBuffer ||
         Array.isArray(data) || typeof data.length === "number")) {
      return Buffer.from(data);
    }
    const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };

  class Hash extends Transform {
    constructor(algorithm, options) {
      super();
      if (typeof algorithm !== "string") {
        const error = new TypeError(`The "algorithm" argument must be of type string. ${receivedArgument(algorithm)}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const normalizedName = algorithm.toLowerCase();
      const name = normalizedName === "rsa-sha1" ? "sha1" : normalizedName;
      const outputLength = options?.outputLength;
      if ((name === "shake128" || name === "shake256") && outputLength === undefined) {
        const error = new Error("error:030000D6:digital envelope routines::not XOF or invalid length");
        error.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
        throw error;
      }
      if (outputLength !== undefined && typeof outputLength !== "number") {
        const error = new TypeError('The "outputLength" argument must be of type number');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (outputLength !== undefined &&
          (!Number.isInteger(outputLength) || outputLength < 0 || outputLength > 0x7fffffff)) {
        const error = new Error("The value of \"outputLength\" is out of range");
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      const standardLength = {
        md5: 16, ripemd160: 20, sha1: 20, sha224: 28, sha256: 32,
        sha384: 48, sha512: 64, "sha3-224": 28, "sha3-256": 32,
        "sha3-384": 48, "sha3-512": 64, blake2b512: 64, blake2s256: 32,
      }[name];
      if (outputLength !== undefined && name !== "shake128" && name !== "shake256" &&
          standardLength !== undefined && outputLength !== standardLength) {
        const error = new Error("error:030000D6:digital envelope routines::not XOF or invalid length");
        error.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
        throw error;
      }
      if (!getHashes().includes(algorithm) && !getHashes().includes(name)) {
        const error = new Error("Digest method not supported");
        error.code = "ERR_OSSL_EVP_UNSUPPORTED";
        throw error;
      }
      const defaultEncoding = options?.defaultEncoding ?? "utf8";
      const state = { name, chunks: [], lifecycle: "open", defaultEncoding, outputLength };
      states.set(this, state);
      this[kHandle] = { update: () => true, digest: () => Buffer.from([]) };
      this._writableState.defaultEncoding = defaultEncoding;
      this._transform = (chunk, encoding, callback) => {
        try {
          this.update(chunk, encoding);
          callback();
        } catch (error) {
          callback(error);
        }
      };
      this._flush = (callback) => {
        try {
          const digest = this.digest();
          state.streamFinalized = true;
          state.streamDigest = digest;
          callback(null, digest);
        } catch (error) {
          callback(error);
        }
      };
    }

    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      if (!this[kHandle].update(data, encoding)) {
        const error = new Error("Hash update failed");
        error.code = "ERR_CRYPTO_HASH_UPDATE_FAILED";
        throw error;
      }
      const bytes = inputBuffer(data, encoding);
      state.chunks.push(Array.from(bytes));
      return this;
    }

    digest(encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") {
        if (state.streamFinalized) {
          return encoding === undefined || encoding === "buffer"
            ? state.streamDigest
            : state.streamDigest.toString(String(encoding));
        }
        throw finalized();
      }
      const outputEncoding = encoding === undefined || encoding === "buffer"
        ? undefined
        : String(encoding);
      state.lifecycle = "finalized";
      const input = state.chunks.flat();
      state.chunks = [];
      const bytes = Buffer.from(hashDigest(state.name, input, state.outputLength ?? 0));
      return outputEncoding === undefined
        ? bytes
        : bytes.toString(outputEncoding);
    }

    copy(options) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      if ((state.name === "shake128" || state.name === "shake256") &&
          (options == null || options.outputLength === undefined)) {
        const error = new Error("error:030000D6:digital envelope routines::not XOF or invalid length");
        error.code = "ERR_OSSL_EVP_NOT_XOF_OR_INVALID_LENGTH";
        throw error;
      }
      const outputLength = options?.outputLength ?? state.outputLength;
      const copy = new Hash(state.name, { defaultEncoding: state.defaultEncoding, outputLength });
      states.get(copy).chunks = state.chunks.map((chunk) => chunk.slice());
      return copy;
    }
  }

  function hashOnce(algorithm, data, options) {
    if (typeof algorithm !== "string") {
      const error = new TypeError(`The "algorithm" argument must be of type string. ${receivedArgument(algorithm)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    let outputEncoding = "hex";
    let outputLength;
    if (typeof options === "string") {
      outputEncoding = options;
    } else if (options !== undefined) {
      if (options === null || typeof options !== "object" || Array.isArray(options)) {
        const error = new TypeError('The "options" argument must be of type object or string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      outputEncoding = options.outputEncoding ?? "hex";
      outputLength = options.outputLength;
    }
    if (outputEncoding !== undefined && ![
      "buffer", "hex", "base64", "base64url", "latin1", "binary",
      "ascii", "utf8", "utf-8", "ucs2", "ucs-2", "utf16le",
    ].includes(String(outputEncoding).toLowerCase())) {
      const error = new TypeError(`Unknown encoding: ${outputEncoding}`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (outputLength !== undefined && typeof outputLength !== "number") {
      const error = new TypeError('The "outputLength" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const normalized = algorithm.toLowerCase();
    const standardLength = {
      md5: 16, ripemd160: 20, sha1: 20, "rsa-sha1": 20,
      sha224: 28, sha256: 32, sha384: 48, sha512: 64,
      "sha3-224": 28, "sha3-256": 32, "sha3-384": 48, "sha3-512": 64,
      blake2b512: 64, blake2s256: 32,
    }[normalized];
    if (outputLength !== undefined && standardLength !== undefined &&
        normalized !== "shake128" && normalized !== "shake256" &&
        outputLength !== standardLength) {
      throw new Error(`Output length ${outputLength} is invalid for ${normalized}, which does not support XOF`);
    }
    const hash = new Hash(algorithm, { outputLength });
    hash.update(data);
    return hash.digest(outputEncoding);
  }

  let hashConstructorWarningEmitted = false;
  function HashConstructor(algorithm, options) {
    if (!new.target && !hashConstructorWarningEmitted) {
      hashConstructorWarningEmitted = true;
      process.emitWarning("crypto.Hash constructor is deprecated.", {
        type: "DeprecationWarning",
        code: "DEP0179",
      });
    }
    return new Hash(algorithm, options);
  }
  HashConstructor.prototype = Hash.prototype;

  class Hmac {
    constructor(algorithm, key) {
      if (typeof algorithm !== "string") {
        const error = new TypeError(`The "hmac" argument must be of type string. Received ${String(algorithm)}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const requestedName = algorithm.toLowerCase();
      const name = requestedName === "dss1" ? "sha1" : requestedName;
      if (!getHashes().includes(algorithm) && !getHashes().includes(name)) throw unsupportedDigest(algorithm);
      if (key === undefined || key === null) {
        const error = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const keyBytes = secretKeys.has(key)
        ? Buffer.from(secretKeys.get(key))
        : Buffer.from(key);
      states.set(this, { name, key: Array.from(keyBytes), chunks: [], lifecycle: "open" });
    }

    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      const bytes = inputBuffer(data, encoding);
      state.chunks.push(Array.from(bytes));
      return this;
    }

    digest(encoding) {
      const state = states.get(this);
      const outputEncoding = encoding === undefined || encoding === "buffer"
        ? undefined
        : String(encoding);
      if (state.lifecycle !== "open") {
        if (!repeatedHmacDigestWarningEmitted) {
          repeatedHmacDigestWarningEmitted = true;
          process.emitWarning("Calling Hmac.digest() more than once is deprecated.", {
            type: "DeprecationWarning",
            code: "DEP0206",
          });
        }
        const empty = Buffer.alloc(0);
        return outputEncoding === undefined ? empty : empty.toString(outputEncoding);
      }
      state.lifecycle = "finalized";
      const input = state.chunks.flat();
      state.chunks = [];
      const bytes = Buffer.from(hmacDigest(state.name, state.key, input));
      return outputEncoding === undefined
        ? bytes
        : bytes.toString(outputEncoding);
    }

    end(data, encoding) {
      if (data !== undefined) this.update(data, encoding);
      const state = states.get(this);
      state.streamResult = this.digest();
      return this;
    }

    read() {
      const state = states.get(this);
      const result = state.streamResult;
      state.streamResult = undefined;
      return result;
    }
  }

  let hmacConstructorWarningEmitted = false;
  function HmacConstructor(algorithm, key) {
    if (!new.target && !hmacConstructorWarningEmitted) {
      hmacConstructorWarningEmitted = true;
      process.emitWarning("crypto.Hmac constructor is deprecated.", {
        type: "DeprecationWarning",
        code: "DEP0181",
      });
    }
    return new Hmac(algorithm, key);
  }
  HmacConstructor.prototype = Hmac.prototype;

  let setEngineWarningEmitted = false;
  function setEngine(id, flags) {
    if (typeof id !== "string") {
      const error = new TypeError('The "id" argument must be of type string');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (flags !== undefined && typeof flags !== "number") {
      const error = new TypeError('The "flags" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!setEngineWarningEmitted) {
      setEngineWarningEmitted = true;
      process.emitWarning("OpenSSL engine-based APIs are deprecated.", {
        type: "DeprecationWarning",
        code: "DEP0183",
      });
    }
    const error = new Error("Engine-based crypto is not supported");
    error.code = "ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED";
    throw error;
  }

  class Sign {
    constructor(algorithm) {
      if (typeof algorithm !== "string") {
        const error = new TypeError('The "algorithm" argument must be of type string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const name = algorithm.toLowerCase();
      if (!getHashes().includes(algorithm) && !getHashes().includes(name)) throw unsupportedDigest(algorithm);
      states.set(this, { name, chunks: [], lifecycle: "open" });
    }

    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      state.chunks.push(Array.from(inputBuffer(data, encoding)));
      return this;
    }

    end(data, encoding) {
      if (data !== undefined) this.update(data, encoding);
      return this;
    }

    sign(key, outputEncoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      state.lifecycle = "finalized";
      const material = key !== null && typeof key === "object" && key.key !== undefined ? key.key : key;
      const pem = typeof material === "string" ? Buffer.from(material) : inputBuffer(material);
      try {
        const signature = Buffer.from(signDigest(state.name, state.chunks.flat(), Array.from(pem)));
        return outputEncoding === undefined || outputEncoding === "buffer"
          ? signature
          : signature.toString(outputEncoding);
      } catch (cause) {
        if (String(cause).includes("digest too big for rsa key")) {
          const error = new Error("error:02000070:rsa routines::digest too big for rsa key");
          error.library = "rsa routines";
          throw error;
        }
        throw cause;
      }
    }
  }

  class Verify {
    constructor(algorithm) {
      if (typeof algorithm !== "string") {
        const error = new TypeError('The "algorithm" argument must be of type string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      const name = algorithm.toLowerCase();
      if (!getHashes().includes(algorithm) && !getHashes().includes(name)) throw unsupportedDigest(algorithm);
      states.set(this, { name, chunks: [], lifecycle: "open" });
    }
    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      state.chunks.push(Array.from(inputBuffer(data, encoding)));
      return this;
    }
    end(data, encoding) {
      if (data !== undefined) this.update(data, encoding);
      return this;
    }
    verify(key, signature, signatureEncoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized();
      state.lifecycle = "finalized";
      let material = key;
      if (key !== null && typeof key === "object" && !ArrayBuffer.isView(key) && !(key instanceof ArrayBuffer)) {
        if (key.padding !== undefined && typeof key.padding !== "number") {
          const error = new TypeError(`The property 'options.padding' is invalid. Received ${String(key.padding)}`);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (key.saltLength !== undefined && typeof key.saltLength !== "number") {
          const error = new TypeError(`The property 'options.saltLength' is invalid. Received ${String(key.saltLength)}`);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        material = key.key;
      }
      const pem = typeof material === "string" ? Buffer.from(material) : inputBuffer(material);
      const signatureBytes = typeof signature === "string"
        ? Buffer.from(signature, signatureEncoding)
        : inputBuffer(signature);
      return verifyDigest(state.name, state.chunks.flat(), Array.from(pem), Array.from(signatureBytes));
    }
  }

  function SignConstructor(algorithm) { return new Sign(algorithm); }
  SignConstructor.prototype = Sign.prototype;
  function VerifyConstructor(algorithm) { return new Verify(algorithm); }
  VerifyConstructor.prototype = Verify.prototype;
  const signOnce = (algorithm, data, key, options) => {
    const signer = new Sign(algorithm);
    signer.update(data);
    const outputEncoding = typeof options === "string" ? options : undefined;
    return signer.sign(key, outputEncoding);
  };
  const verifyOnce = (algorithm, data, key, signature, options) => {
    const verifier = new Verify(algorithm);
    verifier.update(data);
    const material = options === undefined ? key : Object.assign({}, options, { key });
    return verifier.verify(material, signature);
  };

  const hashNames = Object.freeze([
    "RSA-SHA1", "blake2b512", "blake2s256", "md5", "ripemd160",
    "sha1", "sha224", "sha256", "sha384", "sha512", "shake128", "shake256",
    "sha3-224", "sha3-256", "sha3-384", "sha3-512",
  ].sort());
  const cipherNames = Object.freeze([
    "aes-128-cbc", "aes-128-ecb", "aes-128-gcm", "aes-192-gcm", "aes-256-cbc", "aes-256-gcm", "aes256", "chacha20-poly1305", "des-ede3-cbc",
  ].sort());
  const curveNames = Object.freeze([
    "prime192v1", "secp224r1", "secp256k1", "secp256r1",
    "secp384r1", "secp521r1",
  ].sort());
  function getHashes() { return hashNames.slice(); }
  function getCiphers() { return cipherNames.slice(); }
  function getCurves() { return curveNames.slice(); }
  const cipherInfoRecords = [
    { name: "aes-128-cbc", nid: 419, blockSize: 16, ivLength: 16, keyLength: 16, mode: "cbc" },
    { name: "aes-128-ecb", nid: 418, blockSize: 16, ivLength: 0, keyLength: 16, mode: "ecb" },
    { name: "aes-256-cbc", nid: 427, blockSize: 16, ivLength: 16, keyLength: 32, mode: "cbc" },
    { name: "aes-192-gcm", nid: 898, blockSize: 1, ivLength: 12, keyLength: 24, mode: "gcm" },
    { name: "aes-256-gcm", nid: 901, blockSize: 1, ivLength: 12, keyLength: 32, mode: "gcm" },
    { name: "chacha20-poly1305", nid: 1018, blockSize: 1, ivLength: 12, keyLength: 32, mode: "stream" },
    { name: "des-ede3-cbc", nid: 44, blockSize: 8, ivLength: 8, keyLength: 24, mode: "cbc" },
    { name: "aes-128-gcm", nid: 895, blockSize: 1, ivLength: 12, keyLength: 16, mode: "gcm" },
    { name: "aes-128-ccm", nid: 896, blockSize: 1, ivLength: 12, keyLength: 16, mode: "ccm" },
    { name: "aes-128-ocb", nid: 958, blockSize: 1, ivLength: 12, keyLength: 16, mode: "ocb" },
  ];
  function getCipherInfo(nameOrNid, options = {}) {
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const validLength = (value, property) => {
      if (value === undefined) return;
      if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > 0xffffffff) {
        const error = new TypeError(`The "options.${property}" argument must be a uint32`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    };
    validLength(options.keyLength, "keyLength");
    validLength(options.ivLength, "ivLength");
    let info;
    if (typeof nameOrNid === "string") {
      if (!nameOrNid) return undefined;
      const requested = nameOrNid.toLowerCase();
      const name = requested === "aes256" ? "aes-256-cbc" : requested;
      info = cipherInfoRecords.find(record => record.name === name);
    } else if (typeof nameOrNid === "number") {
      if (!Number.isInteger(nameOrNid) || nameOrNid < 1 || nameOrNid > 0x7fffffff) return undefined;
      info = cipherInfoRecords.find(record => record.nid === nameOrNid);
    } else {
      const error = new TypeError('The "nameOrNid" argument must be of type string or number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!info || (options.keyLength !== undefined && options.keyLength !== info.keyLength)) return undefined;
    if (options.ivLength !== undefined) {
      const ivLength = options.ivLength;
      if (info.mode === "ccm" && (ivLength < 7 || ivLength > 13)) return undefined;
      if (info.mode === "ocb" && (ivLength < 1 || ivLength > 15)) return undefined;
      if (info.mode !== "ccm" && info.mode !== "ocb" && ivLength !== info.ivLength) return undefined;
      return { ...info, ivLength };
    }
    return { ...info };
  }
  class CipherBase extends Transform {
    constructor(name, key, iv, decrypt, options = {}) {
      super();
      this._cipherName = name;
      this._cipherKey = Buffer.from(secretKeys.has(key) ? secretKeys.get(key) : key);
      this._cipherIv = Buffer.from(iv);
      this._cipherDecrypt = decrypt;
      this._cipherChunks = [];
      this._cipherBytesEmitted = 0;
      this._cipherFinalized = false;
      this._autoPadding = true;
      this._cipherInputEncoding = undefined;
      this._cipherOutputEncoding = undefined;
      this._cipherMode = getCipherInfo(name)?.mode;
      this._authTagLength = options.authTagLength ?? 16;
      this._authTagLengthExplicit = options.authTagLength !== undefined;
      this._authTagSet = false;
      this._authTagLengthStrictDefault = this._cipherName === "chacha20-poly1305" && !this._authTagLengthExplicit;
      this._cipherAad = Buffer.alloc(0);
      this._authTag = undefined;
      this._cipherDataStarted = false;
      this._transform = (chunk, encoding, callback) => {
        try { callback(null, this.update(chunk)); }
        catch (error) { callback(error); }
      };
      this._flush = (callback) => {
        try { callback(null, this.final()); }
        catch (error) { callback(error); }
      };
    }
    _process(finalBlock) {
      const input = Buffer.concat(this._cipherChunks);
      let output;
      try {
        const processed = cipherProcess(
          this._cipherName, this._cipherKey, this._cipherIv, input,
          finalBlock, this._cipherDecrypt, this._cipherAad,
          this._authTag ?? Buffer.alloc(0), this._authTagLength, this._autoPadding,
        );
        output = Buffer.from(processed.output);
        if (finalBlock && (this._cipherMode === "gcm" || this._cipherName === "chacha20-poly1305") && !this._cipherDecrypt)
          this._authTag = Buffer.from(processed.tag);
      } catch (cause) {
        if ((this._cipherMode === "gcm" || this._cipherName === "chacha20-poly1305") && this._cipherDecrypt && finalBlock) {
          const error = new Error("Unsupported state or unable to authenticate data");
          error.code = "ERR_CRYPTO_AUTH_TAG_MISMATCH";
          throw error;
        }
        if (this._cipherDecrypt && finalBlock) {
          const error = new Error("error:1C800064:Provider routines::bad decrypt");
          error.library = "Provider routines";
          error.reason = "bad decrypt";
          error.code = "ERR_OSSL_BAD_DECRYPT";
          throw error;
        }
        if (!this._cipherDecrypt && finalBlock && !this._autoPadding) {
          const error = new Error("error:1C80006B:Provider routines::wrong final block length");
          error.library = "Provider routines";
          error.reason = "wrong final block length";
          error.code = "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH";
          throw error;
        }
        throw cause;
      }
      const result = output.subarray(this._cipherBytesEmitted);
      this._cipherBytesEmitted = output.length;
      return result;
    }
    update(data, inputEncoding, outputEncoding) {
      if (this._cipherFinalized) throw finalized();
      const normalizeEncoding = (encoding) => {
        if (encoding === undefined) return undefined;
        const value = String(encoding).toLowerCase();
        const normalized = value === "utf-8" ? "utf8" : value === "binary" ? "latin1" : value;
        if (!["utf8", "hex", "base64", "base64url", "latin1", "ascii", "ucs2", "ucs-2", "utf16le", "buffer"].includes(normalized)) {
          const error = new TypeError(`Unknown encoding: ${encoding}`);
          error.code = "ERR_UNKNOWN_ENCODING";
          throw error;
        }
        return normalized;
      };
      const checkEncoding = (field, supplied) => {
        if (supplied === undefined) return;
        const normalized = normalizeEncoding(supplied);
        if (this[field] !== undefined && normalized !== this[field]) {
          const error = new TypeError(`The encoding cannot be changed from '${this[field]}'`);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        this[field] = normalized;
      };
      checkEncoding("_cipherInputEncoding", inputEncoding);
      checkEncoding("_cipherOutputEncoding", outputEncoding);
      const inputLength = data?.byteLength ?? data?.length;
      if (typeof inputLength === "number" && inputLength > 0x7fffffff - 16) {
        const error = new RangeError("The data exceeds the maximum supported size");
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      this._cipherChunks.push(inputBuffer(data, inputEncoding));
      this._cipherDataStarted = true;
      const result = this._process(false);
      return outputEncoding === undefined || outputEncoding === "buffer"
        ? result
        : result.toString(outputEncoding);
    }
    final(outputEncoding) {
      if (this._cipherFinalized) throw finalized();
      if (outputEncoding !== undefined) {
        const value = String(outputEncoding).toLowerCase();
        const normalized = value === "utf-8" ? "utf8" : value === "binary" ? "latin1" : value;
        if (!["utf8", "hex", "base64", "base64url", "latin1", "ascii", "ucs2", "ucs-2", "utf16le", "buffer"].includes(normalized)) {
          const error = new TypeError(`Unknown encoding: ${outputEncoding}`);
          error.code = "ERR_UNKNOWN_ENCODING";
          throw error;
        }
        if (this._cipherOutputEncoding !== undefined && normalized !== this._cipherOutputEncoding &&
            !((this._cipherMode === "gcm" || this._cipherName === "chacha20-poly1305") && this._cipherDecrypt)) {
          const error = new TypeError(`The encoding cannot be changed from '${this._cipherOutputEncoding}'`);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        this._cipherOutputEncoding = normalized;
      }
      this._cipherFinalized = true;
      const result = this._process(true);
      return outputEncoding === undefined || outputEncoding === "buffer"
        ? result
        : result.toString(outputEncoding);
    }
    setAutoPadding(autoPadding = true) {
      this._autoPadding = Boolean(autoPadding);
      return this;
    }
    setAAD(aad) {
      if ((this._cipherMode !== "gcm" && this._cipherName !== "chacha20-poly1305") || this._cipherDataStarted || this._cipherFinalized)
        throw new Error("Invalid state for operation setAAD");
      this._cipherAad = inputBuffer(aad);
      return this;
    }
    setAuthTag(tag) {
      if ((this._cipherMode !== "gcm" && this._cipherName !== "chacha20-poly1305") || !this._cipherDecrypt || this._cipherFinalized || this._authTagSet)
        throw new Error("Invalid state for operation setAuthTag");
      const bytes = inputBuffer(tag);
      const validLength = this._cipherName === "chacha20-poly1305"
        ? bytes.length >= 1 && bytes.length <= 16
        : [4, 8, 12, 13, 14, 15, 16].includes(bytes.length);
      if (!validLength || ((this._authTagLengthExplicit || this._authTagLengthStrictDefault) && bytes.length !== this._authTagLength)) {
        const error = new TypeError(`Invalid authentication tag length: ${bytes.length}`);
        error.code = (this._cipherName === "chacha20-poly1305" || this._cipherMode === "gcm")
          ? "ERR_CRYPTO_INVALID_AUTH_TAG"
          : "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      this._authTagLength = bytes.length;
      this._authTag = bytes;
      this._authTagSet = true;
      return this;
    }
    getAuthTag() {
      if ((this._cipherMode !== "gcm" && this._cipherName !== "chacha20-poly1305") || this._cipherDecrypt || !this._cipherFinalized || !this._authTag)
        throw new Error("Invalid state for operation getAuthTag");
      return this._authTag;
    }
  }
  function cipherOptions(options) {
    if (options === undefined) return {};
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return options;
  }
  function validateTagLength(options, chacha = false) {
    const length = options.authTagLength ?? 16;
    if (chacha && (typeof length !== "number" || !Number.isInteger(length) || length < 1 || length > 16)) {
      const error = new Error("Invalid authentication tag length");
      error.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
      throw error;
    }
    if (!chacha && (typeof length !== "number" || !Number.isInteger(length) || ![4, 8, 12, 13, 14, 15, 16].includes(length))) {
      const error = new TypeError("Invalid authentication tag length");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return { authTagLength: length };
  }
  function createCipheriv(name, key, iv, options) {
    if (typeof name !== "string") {
      const error = new TypeError(`The "cipher" argument must be of type string. ${receivedArgument(name)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    options = cipherOptions(options);
    const info = getCipherInfo(name);
    if (info === undefined) {
      const error = new Error("Unknown cipher");
      error.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
      throw error;
    }
    const bytesLike = (value) => typeof value === "string" || secretKeys.has(value) ||
      ArrayBuffer.isView(value) || value instanceof ArrayBuffer ||
      typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer;
    if (!bytesLike(key)) {
      const error = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const keyBytes = Buffer.from(secretKeys.has(key) ? secretKeys.get(key) : key);
    if (keyBytes.length !== info.keyLength) throw new TypeError("Invalid key length");
    if (iv === undefined) {
      const error = new TypeError('The "iv" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (iv !== null && !bytesLike(iv)) {
      const error = new TypeError('The "iv" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (iv === null && info.ivLength !== 0) throw new Error("Invalid initialization vector");
    const ivBytes = iv === null && info.ivLength === 0 ? Buffer.alloc(0) : Buffer.from(iv);
    if (info.mode === "gcm" ? ivBytes.length === 0 : ivBytes.length !== info.ivLength) {
      throw new Error("Invalid initialization vector");
    }
    return new CipherBase(info.name, keyBytes, ivBytes, false, (info.mode === "gcm" || info.name === "chacha20-poly1305") ? validateTagLength(options, info.name === "chacha20-poly1305") : options);
  }
  function createDecipheriv(name, key, iv, options) {
    if (typeof name !== "string") {
      const error = new TypeError(`The "cipher" argument must be of type string. ${receivedArgument(name)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    options = cipherOptions(options);
    const info = getCipherInfo(name);
    if (info === undefined) {
      const error = new Error("Unknown cipher");
      error.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
      throw error;
    }
    const bytesLike = (value) => typeof value === "string" || secretKeys.has(value) ||
      ArrayBuffer.isView(value) || value instanceof ArrayBuffer ||
      typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer;
    if (!bytesLike(key)) {
      const error = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const keyBytes = Buffer.from(secretKeys.has(key) ? secretKeys.get(key) : key);
    if (keyBytes.length !== info.keyLength) throw new TypeError("Invalid key length");
    if (iv === undefined) {
      const error = new TypeError('The "iv" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (iv !== null && !bytesLike(iv)) {
      const error = new TypeError('The "iv" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (iv === null && info.ivLength !== 0) throw new Error("Invalid initialization vector");
    const ivBytes = iv === null && info.ivLength === 0 ? Buffer.alloc(0) : Buffer.from(iv);
    if (info.mode === "gcm" ? ivBytes.length === 0 : ivBytes.length !== info.ivLength) {
      throw new Error("Invalid initialization vector");
    }
    return new CipherBase(info.name, keyBytes, ivBytes, true, (info.mode === "gcm" || info.name === "chacha20-poly1305") ? validateTagLength(options, info.name === "chacha20-poly1305") : options);
  }
  function Cipheriv(name, key, iv) { return createCipheriv(name, key, iv); }
  function Decipheriv(name, key, iv) { return createDecipheriv(name, key, iv); }
  Cipheriv.prototype = CipherBase.prototype;
  Decipheriv.prototype = CipherBase.prototype;
  function createSecretKey(key) {
    const bytes = Buffer.from(key);
    const result = Object.create(null);
    secretKeys.set(result, Array.from(bytes));
    Object.defineProperties(result, {
      type: { value: "secret", enumerable: true },
      symmetricKeySize: { get() { return secretKeys.get(this).length; }, enumerable: true },
      export: { value() { return Buffer.from(secretKeys.get(this)); } },
    });
    return result;
  }

  const bindAsyncCallback = (callback) => {
    if (typeof callback !== "function") return callback;
    const domain = process.domain;
    return domain ? domain.bind(callback) : callback;
  };
  const receivedArgument = (value) => {
    if (value === null) return "Received null";
    if (value === undefined) return "Received undefined";
    if (typeof value === "string") return `Received type string ('${value}')`;
    if (typeof value === "number") return `Received type number (${value})`;
    if (typeof value === "boolean") return `Received type boolean (${value})`;
    if (typeof value === "object") return `Received an instance of ${Array.isArray(value) ? "Array" : value.constructor?.name || "Object"}`;
    return `Received type ${typeof value}`;
  };
  const validateRandomSize = (size, name = "size", maximum = 0x7fffffff) => {
    if (typeof size !== "number") {
      const error = new TypeError(`The "${name}" argument must be of type number. ${receivedArgument(size)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isFinite(size) || size < 0 || size > maximum) {
      const error = new RangeError(`The value of "${name}" is out of range. It must be >= 0 && <= ${maximum}. Received ${size}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return Math.floor(size);
  };
  const argumentTypeError = (name, value) => {
    let detail;
    if (value === null || value === undefined) detail = ` Received ${value}`;
    else if (typeof value === "object") {
      const type = Array.isArray(value) ? "Array" : value.constructor?.name;
      detail = type ? ` Received an instance of ${type}` : " Received an object";
    } else if (typeof value === "function") {
      detail = ` Received function ${value.name}`;
    } else {
      const inspected = typeof value === "string" ? `'${value}'` : String(value);
      detail = ` Received type ${typeof value} (${inspected})`;
    }
    const error = new TypeError(`The "${name}" argument must be of type number.${detail}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };
  const rangeError = (name, value) => {
    const detail = !Number.isInteger(value)
      ? ` It must be an integer. Received ${value}`
      : ` Received ${value}`;
    const error = new RangeError(`The value of "${name}" is out of range.${detail}`);
    error.code = "ERR_OUT_OF_RANGE";
    return error;
  };
  const invalidDigest = (digest) => {
    const error = new TypeError(`Invalid digest: ${digest}`);
    error.code = "ERR_CRYPTO_INVALID_DIGEST";
    return error;
  };
  const cryptoBytes = (value) => typeof value === "string"
    ? Buffer.from(value)
    : ArrayBuffer.isView(value)
    ? Buffer.from(value.buffer, value.byteOffset, value.byteLength)
    : Buffer.from(value);
  const pbkdf2Arguments = (password, salt, iterations, keylen, digest) => {
    const bytesLike = (value) => typeof value === "string" ||
      value instanceof ArrayBuffer ||
      typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer ||
      ArrayBuffer.isView(value);
    if (!bytesLike(password) || !bytesLike(salt)) {
      const error = new TypeError('The "password" and "salt" arguments must be strings or ArrayBuffer views');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (typeof iterations !== "number") throw argumentTypeError("iterations", iterations);
    if (typeof keylen !== "number") throw argumentTypeError("keylen", keylen);
    if (!Number.isInteger(iterations) || iterations < 1 || iterations > 0x7fffffff) {
      throw rangeError("iterations", iterations);
    }
    if (!Number.isInteger(keylen) || keylen < 0 || keylen > 0x7fffffff) {
      throw rangeError("keylen", keylen);
    }
    if (typeof digest !== "string") {
      const error = new TypeError(`The "digest" argument must be of type string. Received ${digest === null ? "null" : String(digest)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!getHashes().includes(digest) && !getHashes().includes(digest.toLowerCase())) {
      throw invalidDigest(digest);
    }
  };
  const randomBuffer = (size, callback) => {
    size = validateRandomSize(size);
    const bytes = Buffer.from(randomBytes(size));
    if (callback === undefined) return bytes;
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    process.nextTick(bindAsyncCallback(callback), null, bytes);
    return undefined;
  };
  let pseudoRandomWarningEmitted = false;
  const pseudoRandomBuffer = (size, callback) => {
    if (!pseudoRandomWarningEmitted) {
      pseudoRandomWarningEmitted = true;
      process.emitWarning("crypto.pseudoRandomBytes is deprecated.", {
        type: "DeprecationWarning",
        code: "DEP0115",
      });
    }
    return randomBuffer(size, callback);
  };
  const derivePbkdf2 = (password, salt, iterations, keylen, digest, callback) => {
    if (typeof digest === "function" && callback === undefined) {
      callback = digest;
      digest = undefined;
    }
    pbkdf2Arguments(password, salt, iterations, keylen, digest);
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const key = Buffer.from(pbkdf2(
      cryptoBytes(password), cryptoBytes(salt), iterations, keylen, digest,
    ));
    process.nextTick(bindAsyncCallback(callback), null, key);
  };
  const derivePbkdf2Sync = (password, salt, iterations, keylen, digest) => {
    pbkdf2Arguments(password, salt, iterations, keylen, digest);
    return Buffer.from(pbkdf2(cryptoBytes(password), cryptoBytes(salt), iterations, keylen, digest));
  };
  const scryptBytes = (value, name) => {
    const bytesLike = typeof value === "string" || secretKeys.has(value) ||
      value instanceof ArrayBuffer || typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer ||
      ArrayBuffer.isView(value);
    if (!bytesLike) {
      const error = new TypeError(`The "${name}" argument must be a string or an ArrayBuffer view`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return secretKeys.has(value) ? Buffer.from(secretKeys.get(value)) : cryptoBytes(value);
  };
  const scryptArgs = (password, salt, keylen, options) => {
    const pass = scryptBytes(password, "password");
    const saltBytes = scryptBytes(salt, "salt");
    if (typeof keylen !== "number") {
      const error = new TypeError('The "keylen" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isSafeInteger(keylen) || keylen < 0 || keylen > 0x7fffffff) {
      const error = new RangeError('The "keylen" argument is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    let N = options.N;
    let cost = options.cost;
    let r = options.r;
    let blockSize = options.blockSize;
    let p = options.p;
    let parallelization = options.parallelization;
    let maxmem = options.maxmem;
    if (N !== undefined && cost !== undefined) {
      const error = new TypeError('Option "N" cannot be used in combination with option "cost"');
      error.code = "ERR_INCOMPATIBLE_OPTION_PAIR";
      throw error;
    }
    if (p !== undefined && parallelization !== undefined) {
      const error = new TypeError('Option "p" cannot be used in combination with option "parallelization"');
      error.code = "ERR_INCOMPATIBLE_OPTION_PAIR";
      throw error;
    }
    if (r !== undefined && blockSize !== undefined) {
      const error = new TypeError('Option "r" cannot be used in combination with option "blockSize"');
      error.code = "ERR_INCOMPATIBLE_OPTION_PAIR";
      throw error;
    }
    N = N ?? cost ?? 16384;
    r = r ?? blockSize ?? 8;
    p = p ?? parallelization ?? 1;
    maxmem = maxmem ?? 32 * 1024 * 1024;
    const integerOption = (value, name, max = Number.MAX_SAFE_INTEGER) => {
      if (typeof value !== "number") {
        const error = new TypeError(`The "${name}" argument must be of type number`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!Number.isSafeInteger(value) || value < 0 || value > max) {
        const error = new RangeError(`The "${name}" argument is out of range`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      return value;
    };
    N = integerOption(N, "N");
    r = integerOption(r, "r");
    p = integerOption(p, "p");
    maxmem = integerOption(maxmem, "maxmem");
    if (N < 2 || (N & (N - 1)) !== 0 || r === 0 || p === 0 ||
        N >= 2 ** (r * 16) || p > (2 ** 30 - 1) / r) {
      const error = new Error("Invalid scrypt params: memory limit exceeded");
      error.code = "ERR_CRYPTO_INVALID_SCRYPT_PARAMS";
      throw error;
    }
    const requiredMemory = 128 * N * r + 128 * r * p + 256 * r;
    if (requiredMemory > maxmem) {
      const error = new Error("Invalid scrypt params: memory limit exceeded");
      error.code = "ERR_CRYPTO_INVALID_SCRYPT_PARAMS";
      throw error;
    }
    return { password: pass, salt: saltBytes, keylen, N, r, p, maxmem };
  };
  const scryptResult = (args) => {
    try {
      return Buffer.from(scryptNative(args.password, args.salt, args.N, args.r, args.p, args.maxmem, args.keylen));
    } catch (cause) {
      const error = new Error(`Invalid scrypt params: ${cause?.message || cause}`);
      error.code = "ERR_CRYPTO_INVALID_SCRYPT_PARAMS";
      throw error;
    }
  };
  const scryptSync = (password, salt, keylen, options) => {
    if (options === undefined) options = {};
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return scryptResult(scryptArgs(password, salt, keylen, options));
  };
  const scrypt = (password, salt, keylen, options, callback) => {
    if (typeof options === "function") { callback = options; options = {}; }
    if (options === undefined) options = {};
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const args = scryptArgs(password, salt, keylen, options);
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    process.nextTick(bindAsyncCallback(callback), null, scryptResult(args));
  };
  const hkdfInputs = (digest, ikm, salt, info, length) => {
    if (typeof digest !== "string") {
      const error = new TypeError('The "digest" argument must be of type string');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const bytes = (value, name) => {
      if (!(typeof value === "string" || secretKeys.has(value) || ArrayBuffer.isView(value) ||
          value instanceof ArrayBuffer || typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer)) {
        const error = new TypeError(`The "${name}" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (secretKeys.has(value)) return Buffer.from(secretKeys.get(value));
      if (typeof value === "string") return Buffer.from(value);
      if (ArrayBuffer.isView(value)) return Buffer.from(new Uint8Array(value.buffer, value.byteOffset, value.byteLength));
      return Buffer.from(new Uint8Array(value));
    };
    const inputKey = bytes(ikm, "ikm");
    const inputSalt = bytes(salt, "salt");
    const inputInfo = bytes(info, "info");
    if (inputInfo.length > 1024) {
      const error = new RangeError('The "info" argument must be less than or equal to 1024 bytes');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (typeof length !== "number") {
      const error = new TypeError('The "length" argument must be of type number');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const maximum = Buffer.constants?.MAX_LENGTH ?? 0x7fffffff;
    if (!Number.isInteger(length) || length < 0 || length > maximum) {
      const error = new RangeError('The "length" argument is out of range');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const normalizedDigest = digest.toLowerCase();
    if (!getHashes().includes(normalizedDigest)) {
      const error = new Error("Invalid digest: " + digest);
      error.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw error;
    }
    const digestLength = ({ md5: 16, ripemd160: 20, sha1: 20, sha224: 28, sha256: 32,
      sha384: 48, sha512: 64, "sha3-224": 28, "sha3-256": 32,
      "sha3-384": 48, "sha3-512": 64 })[normalizedDigest];
    if (digestLength === undefined) {
      const error = new Error("Invalid digest: " + digest);
      error.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw error;
    }
    if (length > digestLength * 255) {
      const error = new RangeError("Invalid key length");
      error.code = "ERR_CRYPTO_INVALID_KEYLEN";
      throw error;
    }
    return { digest: normalizedDigest, ikm: inputKey, salt: inputSalt, info: inputInfo, length, digestLength };
  };
  const hkdfResult = ({ digest, ikm, salt, info, length, digestLength }) => {
    const prk = Buffer.from(hmacDigest(digest, Array.from(salt), Array.from(ikm)));
    const output = Buffer.alloc(length);
    let previous = Buffer.alloc(0);
    let offset = 0;
    for (let counter = 1; offset < length; counter++) {
      previous = Buffer.from(hmacDigest(digest, Array.from(prk), [...previous, ...info, counter]));
      const count = Math.min(digestLength, length - offset);
      previous.copy(output, offset, 0, count);
      offset += count;
    }
    return output.buffer.slice(output.byteOffset, output.byteOffset + output.byteLength);
  };
  const hkdfSync = (digest, ikm, salt, info, length) => hkdfResult(hkdfInputs(digest, ikm, salt, info, length));
  const hkdf = (digest, ikm, salt, info, length, callback) => {
    const params = hkdfInputs(digest, ikm, salt, info, length);
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    process.nextTick(bindAsyncCallback(callback), null, hkdfResult(params));
  };
  const randomFillSync = (buffer, offset, size) => {
    const view = ArrayBuffer.isView(buffer)
      ? new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength)
      : buffer instanceof ArrayBuffer || typeof SharedArrayBuffer !== "undefined" && buffer instanceof SharedArrayBuffer
      ? new Uint8Array(buffer)
      : undefined;
    if (!view) {
      const error = new TypeError('The "buffer" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const offsetLimit = ArrayBuffer.isView(buffer) ? buffer.length : view.length;
    offset = offset === undefined ? 0 : validateRandomSize(offset, "offset", offsetLimit);
    const byteOffset = offset * (ArrayBuffer.isView(buffer) ? (buffer.BYTES_PER_ELEMENT || 1) : 1);
    size = size === undefined ? view.length - byteOffset : validateRandomSize(size, "size");
    if (byteOffset + size > view.length) {
      const total = byteOffset + size;
      const error = new RangeError(`The value of "size + offset" is out of range. It must be <= ${view.length}. Received ${total}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const bytes = randomBytes(size);
    for (let i = 0; i < size; i++) view[byteOffset + i] = bytes[i];
    return buffer;
  };
  const randomFill = (buffer, offset, size, callback) => {
    if (typeof offset === "function") { callback = offset; offset = 0; size = undefined; }
    else if (typeof size === "function") { callback = size; size = undefined; }
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    randomFillSync(buffer, offset, size);
    process.nextTick(bindAsyncCallback(callback), null, buffer);
    return undefined;
  };
  const randomUUID = (options) => {
    if (options !== undefined && (options === null || typeof options !== "object" || Array.isArray(options))) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options?.disableEntropyCache !== undefined && typeof options.disableEntropyCache !== "boolean") {
      const error = new TypeError('The "options.disableEntropyCache" property must be of type boolean');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const bytes = randomBuffer(16);
    bytes[6] = bytes[6] & 0x0f | 0x40;
    bytes[8] = bytes[8] & 0x3f | 0x80;
    const hex = bytes.toString("hex");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  };
  const randomUUIDv7 = (options) => {
    if (options !== undefined && (options === null || typeof options !== "object" || Array.isArray(options))) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options?.disableEntropyCache !== undefined && typeof options.disableEntropyCache !== "boolean") {
      const error = new TypeError('The "options.disableEntropyCache" property must be of type boolean');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const bytes = randomBuffer(16);
    let timestamp = Date.now();
    for (let index = 5; index >= 0; index--) {
      bytes[index] = timestamp & 0xff;
      timestamp = Math.floor(timestamp / 256);
    }
    bytes[6] = 0x70 | bytes[6] & 0x0f;
    bytes[8] = 0x80 | bytes[8] & 0x3f;
    const hex = bytes.toString("hex");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  };
  const randomInt = (min, max, callback) => {
    const formatInteger = (value) => Math.abs(value) >= 100000
      ? value.toLocaleString("en-US").replaceAll(",", "_")
      : String(value);
    let shorthand = false;
    if (typeof max === "function") { callback = max; max = min; min = 0; shorthand = true; }
    if (max === undefined) { max = min; min = 0; shorthand = true; }
    if (typeof callback !== "undefined" && typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isSafeInteger(min)) {
      const error = new TypeError(`The "min" argument must be a safe integer. ${receivedArgument(min)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isSafeInteger(max)) {
      const error = new TypeError(`The "max" argument must be a safe integer. ${receivedArgument(max)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const maxRange = 0xffffffffffff;
    if (shorthand && max > maxRange) {
      const error = new RangeError(`The value of "max" is out of range. It must be <= ${maxRange}. Received ${formatInteger(max)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const range = max - min;
    if (range <= 0) {
      const error = new RangeError(`The value of "max" is out of range. It must be greater than the value of "min" (${min}). Received ${max}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (range > maxRange) {
      const error = new RangeError(`The value of "max - min" is out of range. It must be <= ${maxRange}. Received ${formatInteger(range)}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    const bytes = randomBytes(6);
    let value = 0;
    for (const byte of bytes) value = value * 256 + byte;
    const result = min + value % range;
    if (callback) process.nextTick(bindAsyncCallback(callback), null, result);
    else return result;
  };
  const webCrypto = globalThis.crypto || {};
  if (!globalThis.crypto) {
    Object.defineProperty(globalThis, "crypto", {
      configurable: true,
      enumerable: false,
      writable: true,
      value: webCrypto,
    });
  }
  if (webCrypto) {
    webCrypto.getRandomValues = function(values) {
      if (this !== webCrypto) {
        const error = new TypeError("Illegal invocation");
        error.code = "ERR_INVALID_THIS";
        throw error;
      }
      if (!ArrayBuffer.isView(values) || values instanceof DataView ||
          values instanceof Float32Array || values instanceof Float64Array) {
        const error = new TypeError("The data argument must be an integer-based TypedArray");
        error.name = "TypeMismatchError";
        throw error;
      }
      if (values.byteLength > 65536) {
        const error = new DOMException("The requested length exceeds 65,536 bytes", "QuotaExceededError");
        throw error;
      }
      randomFillSync(values);
      return values;
    };
  }
  const api = {
    Hash: HashConstructor,
    Verify,
    createVerify: (algorithm) => new Verify(algorithm),
    createHash: (algorithm, options) => new Hash(algorithm, options),
    Hmac: HmacConstructor,
    Sign: SignConstructor,
    Verify: VerifyConstructor,
    sign: signOnce,
    verify: verifyOnce,
    hash: hashOnce,
    createHmac: (algorithm, key) => new Hmac(algorithm, key),
    setEngine,
    createSign: (algorithm) => new Sign(algorithm),
    createSecretKey,
    createCipheriv,
    createDecipheriv,
    Cipheriv,
    Decipheriv,
    getCipherInfo,
    getCiphers,
    getCurves,
    getFips: () => 0,
    getHashes,
    randomBytes: randomBuffer,
    pbkdf2: derivePbkdf2,
    pbkdf2Sync: derivePbkdf2Sync,
    scrypt,
    scryptSync,
    hkdf,
    hkdfSync,
    randomFillSync,
    randomFill,
    randomUUID,
    randomUUIDv7,
    randomInt,
  };
  Object.defineProperties(api, {
    pseudoRandomBytes: { configurable: true, writable: true, value: pseudoRandomBuffer },
    prng: { configurable: true, value: randomBuffer },
    rng: { configurable: true, value: randomBuffer },
  });
  return api;
}"#
);

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    transform: RootId,
) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(CRYPTO_FACTORY, "node:crypto/shared.js")?;
    let hash = context.host_function(crate::host::shared_vm::operation("cryptoHash"))?;
    let hmac = context.host_function(crate::host::shared_vm::operation("cryptoHmac"))?;
    let sign = context.host_function(crate::host::shared_vm::operation("cryptoSign"))?;
    let verify = context.host_function(crate::host::shared_vm::operation("cryptoVerify"))?;
    let random_bytes = context
        .host_function(crate::host::shared_vm::operation("cryptoRandomBytes"))?;
    let pbkdf2 = context.host_function(crate::host::shared_vm::operation("cryptoPbkdf2"))?;
    let scrypt = context.host_function(crate::host::shared_vm::operation("cryptoScrypt"))?;
    let cipher_process =
        context.host_function(crate::host::shared_vm::operation("cryptoCipherProcess"))?;
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let undefined = context.undefined();
    context.call_rooted(
        factory,
        undefined,
        &[
            hash,
            hmac,
            sign,
            verify,
            buffer,
            random_bytes,
            pbkdf2,
            scrypt,
            transform,
            cipher_process,
        ],
    )
}

pub(crate) fn random_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(size) = args.first().copied().and_then(|size| context.rooted_value(size)).and_then(|value| value.as_number()) else {
        return Err(type_error(context, "The \"size\" argument must be of type number" )?);
    };
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > 16_777_216.0 {
        return Err(type_error(context, "The \"size\" argument must be a non-negative integer" )?);
    }
    let mut bytes = vec![0_u8; size as usize];
    openssl::rand::rand_bytes(&mut bytes).map_err(|_| RootedError::host("crypto random generation failed"))?;
    let values = bytes.into_iter().map(|byte| context.number(f64::from(byte))).collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn pbkdf2(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let password = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("crypto PBKDF2 password is missing"))?;
    let salt = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto PBKDF2 salt is missing"))?;
    let iterations = args.get(2).copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 1.0 && value.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto PBKDF2 iterations are invalid"))? as usize;
    let key_length = args.get(3).copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto PBKDF2 key length is invalid"))? as usize;
    let digest_name = args
        .get(4)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto PBKDF2 digest is not a string"))?;
    let digest = openssl::hash::MessageDigest::from_name(&digest_name)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let password = byte_array(context, password)?;
    let salt = byte_array(context, salt)?;
    let mut output = vec![0; key_length];
    openssl::pkcs5::pbkdf2_hmac(&password, &salt, iterations, digest, &mut output)
        .map_err(|error| RootedError::host(format!("crypto PBKDF2 failed: {error}")))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn scrypt(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let password = byte_array(context, *args.first().ok_or_else(|| RootedError::host("scrypt password missing"))?)?;
    let salt = byte_array(context, *args.get(1).ok_or_else(|| RootedError::host("scrypt salt missing"))?)?;
    let number = |index: usize, name: &str| -> Result<u64, RootedError> {
        args.get(index)
            .and_then(|root| context.rooted_value(*root))
            .and_then(|value| value.as_number())
            .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
            .map(|value| value as u64)
            .ok_or_else(|| RootedError::host(format!("scrypt {name} is invalid")))
    };
    let n = number(2, "N")?;
    let r = number(3, "r")?;
    let p = number(4, "p")?;
    let maxmem = number(5, "maxmem")?;
    let key_length = usize::try_from(number(6, "key length")?)
        .map_err(|_| RootedError::host("scrypt key length is invalid"))?;
    let mut output = vec![0; key_length];
    openssl::pkcs5::scrypt(&password, &salt, n, r, p, maxmem, &mut output)
        .map_err(|error| RootedError::host(format!("Invalid scrypt params: {error}")))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn cipher_process(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto cipher algorithm is not a string"))?;
    let cipher = match algorithm.to_ascii_lowercase().as_str() {
        "aes-128-cbc" => openssl::symm::Cipher::aes_128_cbc(),
        "aes-128-ecb" => openssl::symm::Cipher::aes_128_ecb(),
        "aes-128-gcm" => openssl::symm::Cipher::aes_128_gcm(),
        "aes-192-gcm" => openssl::symm::Cipher::aes_192_gcm(),
        "aes-256-cbc" => openssl::symm::Cipher::aes_256_cbc(),
        "aes-256-gcm" => openssl::symm::Cipher::aes_256_gcm(),
        "chacha20-poly1305" => openssl::symm::Cipher::chacha20_poly1305(),
        "des-ede3-cbc" => openssl::symm::Cipher::des_ede3_cbc(),
        _ => {
        return Err(RootedError::host(format!("Unknown cipher: {algorithm}")));
        }
    };
    let key = byte_array(
        context,
        *args.get(1).ok_or_else(|| RootedError::host("cipher key is missing"))?,
    )?;
    let iv = byte_array(
        context,
        *args.get(2).ok_or_else(|| RootedError::host("cipher IV is missing"))?,
    )?;
    let input = byte_array(
        context,
        *args.get(3).ok_or_else(|| RootedError::host("cipher input is missing"))?,
    )?;
    let final_block = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let decrypt = args
        .get(5)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let aad = args.get(6).copied().map(|root| byte_array(context, root)).transpose()?.unwrap_or_default();
    let auth_tag = args.get(7).copied().map(|root| byte_array(context, root)).transpose()?.unwrap_or_default();
    let auth_tag_length = args.get(8)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .unwrap_or(16.0) as usize;
    let auto_padding = args.get(9)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let is_gcm = algorithm.to_ascii_lowercase().ends_with("-gcm");
    let is_aead = is_gcm || algorithm.eq_ignore_ascii_case("chacha20-poly1305");
    let mode = if decrypt {
        openssl::symm::Mode::Decrypt
    } else {
        openssl::symm::Mode::Encrypt
    };
    let iv = if cipher.iv_len() == Some(0) { None } else { Some(iv.as_slice()) };
    let mut crypter = openssl::symm::Crypter::new(cipher, mode, &key, iv)
        .map_err(|error| RootedError::host(error.to_string()))?;
    crypter.pad(auto_padding);
    if is_aead {
        if !aad.is_empty() {
            crypter.aad_update(&aad).map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    let mut output = vec![0; input.len() + cipher.block_size()];
    let mut written = crypter
        .update(&input, &mut output)
        .map_err(|error| RootedError::host(error.to_string()))?;
    if final_block {
        if is_aead && decrypt {
            crypter.set_tag(&auth_tag).map_err(|error| RootedError::host(error.to_string()))?;
        }
        written += crypter
            .finalize(&mut output[written..])
            .map_err(|error| RootedError::host(error.to_string()))?;
    }
    output.truncate(written);
    let output_values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let output_array = context.array_rooted(&output_values)?;
    let tag_array = if is_aead && final_block && !decrypt {
        let mut tag = vec![0; auth_tag_length];
        crypter.get_tag(&mut tag).map_err(|error| RootedError::host(error.to_string()))?;
        let values = tag.iter().map(|byte| context.number(f64::from(*byte))).collect::<Vec<_>>();
        context.array_rooted(&values)?
    } else {
        context.array_rooted(&[])?
    };
    let result = context.object_rooted()?;
    set(context, result, "output", output_array)?;
    set(context, result, "tag", tag_array)?;
    Ok(result)
}

pub(crate) fn hash(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args.first().copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto hash algorithm is not a string"))?;
    let Some(input) = args.get(1).copied() else {
        return Err(type_error(context, "The hash input must be an array of bytes")?);
    };
    let bytes = byte_array(context, input)?;
    let output_length = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .unwrap_or(0.0) as usize;
    let digest = match algorithm.to_ascii_lowercase().as_str() {
        "shake128" | "shake256" => {
            let algorithm = if algorithm.eq_ignore_ascii_case("shake128") {
                openssl::hash::MessageDigest::shake_128()
            } else {
                openssl::hash::MessageDigest::shake_256()
            };
            let mut hasher = openssl::hash::Hasher::new(algorithm)
                .map_err(|_| RootedError::host("crypto digest initialization failed"))?;
            hasher.update(&bytes)
                .map_err(|_| RootedError::host("crypto digest update failed"))?;
            let mut output = vec![0; output_length];
            hasher.finish_xof(&mut output)
                .map_err(|_| RootedError::host("crypto digest failed"))?;
            output
        }
        "sha1" => crate::modules::crypto_sha1::digest(&bytes),
        _ => {
            let algorithm = openssl::hash::MessageDigest::from_name(&algorithm)
                .ok_or_else(|| RootedError::host("Digest method not supported"))?;
            openssl::hash::hash(algorithm, &bytes)
                .map_err(|_| RootedError::host("crypto digest failed"))?
                .to_vec()
        }
    };
    let values = digest
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn hmac(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args.first().copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto HMAC algorithm is not a string"))?;
    let key_root = args.get(1).copied()
        .ok_or_else(|| RootedError::host("crypto HMAC key is missing"))?;
    let input_root = args.get(2).copied()
        .ok_or_else(|| RootedError::host("crypto HMAC input is missing"))?;
    let key = byte_array(context, key_root)?;
    let input = byte_array(context, input_root)?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    // OpenSSL rejects a zero-length HMAC key. A single zero byte has the same
    // block-padded HMAC key representation as an empty key.
    let openssl_key = if key.is_empty() { vec![0] } else { key };
    let key = openssl::pkey::PKey::hmac(&openssl_key)
        .map_err(|error| RootedError::host(format!("crypto HMAC key is invalid ({} bytes): {error}", openssl_key.len())))?;
    let mut signer = openssl::sign::Signer::new(digest, &key)
        .map_err(|error| RootedError::host(format!("crypto HMAC initialization failed: {error}")))?;
    signer.update(&input)
        .map_err(|_| RootedError::host("crypto HMAC update failed"))?;
    let output = signer.sign_to_vec()
        .map_err(|_| RootedError::host("crypto HMAC failed"))?;
    let values = output.iter().map(|byte| context.number(f64::from(*byte))).collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn verify(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args.first().copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto verification algorithm is not a string"))?;
    let input = byte_array(context, *args.get(1).ok_or_else(|| RootedError::host("crypto verification input is missing"))?)?;
    let key_bytes = byte_array(context, *args.get(2).ok_or_else(|| RootedError::host("crypto public key is missing"))?)?;
    let signature = byte_array(context, *args.get(3).ok_or_else(|| RootedError::host("crypto signature is missing"))?)?;
    let key = match openssl::pkey::PKey::public_key_from_pem(&key_bytes) {
        Ok(key) => key,
        Err(_) => {
            if let Ok(private) = openssl::pkey::PKey::private_key_from_pem(&key_bytes) {
                let public = private.public_key_to_pem()
                    .map_err(|error| RootedError::host(error.to_string()))?;
                openssl::pkey::PKey::public_key_from_pem(&public)
                    .map_err(|error| RootedError::host(error.to_string()))?
            } else {
                openssl::x509::X509::from_pem(&key_bytes)
                    .and_then(|certificate| certificate.public_key())
                    .map_err(|error| RootedError::host(error.to_string()))?
            }
        }
    };
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let mut verifier = openssl::sign::Verifier::new(digest, &key)
        .map_err(|error| RootedError::host(error.to_string()))?;
    verifier.update(&input)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let result = verifier.verify(&signature)
        .map_err(|error| RootedError::host(error.to_string()))?;
    Ok(context.boolean(result))
}

pub(crate) fn sign(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args.first().copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto signature algorithm is not a string"))?;
    let input_root = args.get(1).copied()
        .ok_or_else(|| RootedError::host("crypto signature input is missing"))?;
    let key_root = args.get(2).copied()
        .ok_or_else(|| RootedError::host("crypto private key is missing"))?;
    let input = byte_array(context, input_root)?;
    let key_bytes = byte_array(context, key_root)?;
    let key = openssl::pkey::PKey::private_key_from_pem(&key_bytes)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let mut signer = openssl::sign::Signer::new(digest, &key)
        .map_err(|error| RootedError::host(error.to_string()))?;
    signer.update(&input)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let output = signer.sign_to_vec()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let values = output.iter().map(|byte| context.number(f64::from(*byte))).collect::<Vec<_>>();
    context.array_rooted(&values)
}

fn byte_array(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
) -> Result<Vec<u8>, RootedError> {
    if let Some(bytes) = context.view_bytes_rooted(root) {
        return Ok(bytes);
    }
    let length = get(context, root, "length")?;
    let length = context.rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto byte input is not array-like"))? as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let value = get(context, root, &index.to_string())?;
        let byte = context.rooted_value(value)
            .and_then(|value| value.as_number())
            .filter(|byte| byte.is_finite() && *byte >= 0.0 && *byte <= f64::from(u8::MAX))
            .ok_or_else(|| RootedError::host("crypto byte input contained an invalid value"))?;
        bytes.push(byte as u8);
    }
    Ok(bytes)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install crypto property {name}")))
    }
}

fn type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    Ok(context.throw(error))
}
