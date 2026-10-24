//! The hash surface used by the pinned framework package paths.
//!
//! `etag` calls `createHash("sha1").update(string, "utf8").digest("base64")`.
//! Keep that Node-facing wrapper in the guest realm and the digest primitive
//! in Rust; it shares the SHA-1 implementation with the legacy Node module.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::Arc;

const CRYPTO_FACTORY: &str = quench_js_check::checked_js!(
    r#"(hashDigest, hmacDigest, macDigestNative, signDigest, verifyDigest, Buffer, randomBytes, pbkdf2, scryptNative, Transform, cipherProcess, generateRsaKeyPairNative, rsaCryptNative, ecdhNative, diffieHellmanNative, setEngineNative, captureKeyBrands, handleSymbol, getWebCrypto, randomValuesSource, secretMaterial, nativeConstants, getMacsNative, argon2Native, argon2SyncNative, x509Metadata, x509CheckHost) => {
  const states = new WeakMap();
  const secretKeys = new WeakMap();
  let repeatedHmacDigestWarningEmitted = false;
  const unsupportedDigest = (algorithm) => {
    const error = new Error(`Invalid digest: ${algorithm}`);
    error.code = "ERR_OSSL_EVP_UNSUPPORTED";
    error.library = "digital envelope routines";
    error.reason = "unsupported";
    return error;
  };
  const finalized = (state) => {
    const error = new Error(state?.isMac ? "MAC already finalized" : "Digest already called");
    error.code = state?.isMac ? "ERR_CRYPTO_MAC_FINALIZED" : "ERR_CRYPTO_HASH_FINALIZED";
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
        ArrayBuffer.isView(data)) {
      // Crypto update() consumes BufferSource bytes. Buffer.from(typedArray)
      // instead copies its elements (and Buffer.from(DataView) is empty),
      // which is observably different for DataView and wider typed arrays.
      return Buffer.from(data.buffer, data.byteOffset, data.byteLength);
    }
    if (data !== null && typeof data === "object" &&
        (data instanceof ArrayBuffer || Array.isArray(data) ||
         typeof data.length === "number")) {
      return Buffer.from(data);
    }
    const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  };
  const keyBuffer = (key) => ArrayBuffer.isView(key)
    ? Buffer.from(key.buffer, key.byteOffset, key.byteLength)
    : Buffer.from(key);

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
      this[handleSymbol()] = { update: () => true, digest: () => Buffer.from([]) };
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
      if (!this[handleSymbol()].update(data, encoding)) {
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
      const bytes = Buffer.from(hashDigest(state.name, input, state.outputLength));
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

  class Hmac extends Transform {
    constructor(algorithm, key, streamOptions) {
      super(streamOptions);
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
        ? Buffer.from(secretMaterial(secretKeys.get(key)))
        : keyBuffer(key);
      states.set(this, { name, key: Array.from(keyBytes), chunks: [], lifecycle: "open" });
    }

    update(data, encoding) {
      const state = states.get(this);
      if (state.lifecycle !== "open") throw finalized(state);
      const bytes = inputBuffer(data, encoding);
      state.chunks.push(Array.from(bytes));
      return this;
    }

    _transform(chunk, encoding, callback) {
      try {
        this.update(chunk, encoding);
        callback();
      } catch (error) {
        callback(error);
      }
    }

    _flush(callback) {
      try {
        this.push(this.digest());
        callback();
      } catch (error) {
        callback(error);
      }
    }

    digest(encoding) {
      const state = states.get(this);
      const outputEncoding = encoding === undefined || encoding === "buffer"
        ? undefined
        : String(encoding);
      if (state.lifecycle !== "open") {
        if (state.isMac) throw finalized(state);
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
      const bytes = Buffer.from(state.macAlgorithm === "hmac"
        ? hmacDigest(state.name, state.key, input)
        : macDigestNative(state.key, input, state.cipher || "", state.macAlgorithm));
      return outputEncoding === undefined
        ? bytes
        : bytes.toString(outputEncoding);
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
  class Mac extends Hmac {
    constructor(algorithm, key, options) {
      if (algorithm === "cmac") {
        super("sha256", key, options);
        const state = states.get(this);
        state.macAlgorithm = "cmac";
        state.cipher = options.cipher;
      } else if (algorithm === "poly1305") {
        super("sha256", key, options);
        states.get(this).macAlgorithm = "poly1305";
      } else {
        super(options.digest, key, options);
        states.get(this).macAlgorithm = "hmac";
      }
      states.get(this).isMac = true;
    }

    update(data, encoding) {
      if (typeof data !== "string" && !ArrayBuffer.isView(data)) {
        const error = new TypeError('The "data" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (encoding !== undefined && typeof encoding !== "string") {
        const error = new TypeError('The "inputEncoding" argument must be of type string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      // Node only interprets inputEncoding when data is a string. For byte
      // views the second argument is ignored, even when it is not an encoding.
      if (typeof data === "string" && encoding !== undefined && !Buffer.isEncoding(encoding)) {
        const error = new TypeError(`The "encoding" argument is invalid for data of length ${typeof data === "string" ? data.length : data.byteLength}. Received '${encoding}'`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      return super.update(data, encoding);
    }

    final(encoding) {
      if (encoding !== undefined && typeof encoding !== "string") {
        const error = new TypeError('The "outputEncoding" argument must be of type string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (encoding !== undefined && encoding !== "buffer" && !Buffer.isEncoding(encoding)) {
        const error = new TypeError(`The "encoding" argument is invalid. Received '${encoding}'`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      return this.digest(encoding === "buffer" ? undefined : encoding);
    }
  }

  function setEngine(id, flags) { return setEngineNative(id, flags); }

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
      const options = key !== null && typeof key === "object" && key.key !== undefined ? key : {};
      const material = key !== null && typeof key === "object" && key.key !== undefined ? key.key : key;
      const pem = typeof material === "string" ? Buffer.from(material) : inputBuffer(material);
      const passphrase = options.passphrase === undefined ? Buffer.alloc(0) : cryptoBytes(options.passphrase);
      if (pem.toString("utf8").includes("ENCRYPTED") && passphrase.length === 0) {
        throw new Error("error:07880109:common libcrypto routines::interrupted or cancelled");
      }
      try {
        const signature = Buffer.from(signDigest(state.name, state.chunks.flat(), Array.from(pem), Array.from(passphrase), options.format || "pem", options.type || "pkcs8"));
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
      const passphrase = key !== null && typeof key === "object" && key.passphrase !== undefined
        ? cryptoBytes(key.passphrase) : Buffer.alloc(0);
      const keyOptions = key !== null && typeof key === "object" ? key : {};
      return verifyDigest(state.name, state.chunks.flat(), Array.from(pem), Array.from(signatureBytes), Array.from(passphrase), keyOptions.format || "pem", keyOptions.type || "spki");
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
  const timingSafeEqual = (left, right) => {
    const bytes = (value, name) => {
      if (!ArrayBuffer.isView(value)) {
        const error = new TypeError(`The "${name}" argument must be an instance of Buffer, TypedArray, or DataView`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    };
    const a = bytes(left, "buf1");
    const b = bytes(right, "buf2");
    if (a.byteLength !== b.byteLength) {
      const error = new RangeError("Input buffers must have the same byte length");
      error.code = "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH";
      throw error;
    }
    let difference = 0;
    for (let index = 0; index < a.byteLength; index++) difference |= a[index] ^ b[index];
    return difference === 0;
  };

  const hashNames = Object.freeze([
    "RSA-SHA1", "blake2b512", "blake2s256", "md5", "ripemd160",
    "sha1", "sha224", "sha256", "sha384", "sha512", "shake128", "shake256",
    "sha3-224", "sha3-256", "sha3-384", "sha3-512",
  ].sort());
  const cipherNames = Object.freeze([
    "aes-128-cbc", "aes-128-ecb", "aes-128-gcm", "aes-128-ocb", "aes-192-gcm", "aes-192-ocb", "aes-256-cbc", "aes-256-ecb", "aes-256-gcm", "aes-256-ocb", "aes256", "chacha20-poly1305", "des-ede3-cbc",
  ].sort());
  const curveNames = Object.freeze([
    "prime192v1", "secp224r1", "secp256k1", "secp256r1", "prime256v1",
    "secp384r1", "secp521r1",
  ].sort());
  function getHashes() { return hashNames.slice(); }
  function getCiphers() { return cipherNames.slice(); }
  function getCurves() { return curveNames.slice(); }
  const cipherInfoRecords = [
    { name: "aes-128-cbc", nid: 419, blockSize: 16, ivLength: 16, keyLength: 16, mode: "cbc" },
    { name: "aes-128-ecb", nid: 418, blockSize: 16, ivLength: 0, keyLength: 16, mode: "ecb" },
    { name: "aes-256-cbc", nid: 427, blockSize: 16, ivLength: 16, keyLength: 32, mode: "cbc" },
    { name: "aes-256-ecb", nid: 426, blockSize: 16, ivLength: 0, keyLength: 32, mode: "ecb" },
    { name: "aes-192-gcm", nid: 898, blockSize: 1, ivLength: 12, keyLength: 24, mode: "gcm" },
    { name: "aes-192-ocb", nid: 959, blockSize: 16, ivLength: 12, keyLength: 24, mode: "ocb" },
    { name: "aes-256-gcm", nid: 901, blockSize: 1, ivLength: 12, keyLength: 32, mode: "gcm" },
    { name: "aes-256-ocb", nid: 960, blockSize: 16, ivLength: 12, keyLength: 32, mode: "ocb" },
    { name: "chacha20-poly1305", nid: 1018, blockSize: 1, ivLength: 12, keyLength: 32, mode: "stream" },
    { name: "des-ede3-cbc", nid: 44, blockSize: 8, ivLength: 8, keyLength: 24, mode: "cbc" },
    { name: "aes-128-gcm", nid: 895, blockSize: 1, ivLength: 12, keyLength: 16, mode: "gcm" },
    { name: "aes-128-ccm", nid: 896, blockSize: 1, ivLength: 12, keyLength: 16, mode: "ccm" },
    { name: "aes-128-ocb", nid: 958, blockSize: 16, ivLength: 12, keyLength: 16, mode: "ocb" },
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
      return { ...info, ivLength: info.mode === "ecb" ? undefined : ivLength };
    }
    return { ...info, ivLength: info.mode === "ecb" ? undefined : info.ivLength };
  }
  class CipherBase extends Transform {
    constructor(name, key, iv, decrypt, options = {}) {
      super();
      this._cipherName = name;
      this._cipherKey = Buffer.from(secretKeys.has(key) ? secretMaterial(secretKeys.get(key)) : key);
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
        if (finalBlock && (this._cipherMode === "gcm" || this._cipherMode === "ocb" || this._cipherName === "chacha20-poly1305") && !this._cipherDecrypt)
          this._authTag = Buffer.from(processed.tag);
      } catch (cause) {
        if ((this._cipherMode === "gcm" || this._cipherMode === "ocb" || this._cipherName === "chacha20-poly1305") && this._cipherDecrypt && finalBlock) {
          const error = new Error("Unsupported state or unable to authenticate data");
          error.code = "ERR_CRYPTO_AUTH_TAG_MISMATCH";
          throw error;
        }
        if (this._cipherDecrypt && finalBlock) {
          if (input.length === 0) {
            const error = new Error("error:1C80006B:Provider routines::wrong final block length");
            error.library = "Provider routines";
            error.reason = "wrong final block length";
            error.code = "ERR_OSSL_WRONG_FINAL_BLOCK_LENGTH";
            throw error;
          }
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
            !((this._cipherMode === "gcm" || this._cipherMode === "ocb" || this._cipherName === "chacha20-poly1305") && this._cipherDecrypt)) {
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
      if ((this._cipherMode !== "gcm" && this._cipherMode !== "ocb" && this._cipherName !== "chacha20-poly1305") || this._cipherDataStarted || this._cipherFinalized)
        throw new Error("Invalid state for operation setAAD");
      this._cipherAad = inputBuffer(aad);
      return this;
    }
    setAuthTag(tag) {
      if ((this._cipherMode !== "gcm" && this._cipherMode !== "ocb" && this._cipherName !== "chacha20-poly1305") || !this._cipherDecrypt || this._cipherFinalized || this._authTagSet)
        throw new Error("Invalid state for operation setAuthTag");
      const bytes = inputBuffer(tag);
      const validLength = this._cipherMode === "ocb"
        ? bytes.length >= 1 && bytes.length <= 16
        : this._cipherName === "chacha20-poly1305"
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
      if ((this._cipherMode !== "gcm" && this._cipherMode !== "ocb" && this._cipherName !== "chacha20-poly1305") || this._cipherDecrypt || !this._cipherFinalized || !this._authTag)
        throw new Error("Invalid state for operation getAuthTag");
      return this._authTag;
    }
  }
  function cipherOptions(options) {
    if (options === undefined || options === null) return {};
    if (typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return options;
  }
  function validateTagLength(options, chacha = false, ocb = false, cipherName = "") {
    if (ocb && options.authTagLength === undefined) {
      throw new Error(`authTagLength required for ${cipherName}`);
    }
    const length = options.authTagLength ?? 16;
    if (ocb && (typeof length !== "number" || !Number.isInteger(length) || length < 1 || length > 16)) {
      const error = new TypeError("Invalid authentication tag length");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (chacha && (typeof length !== "number" || !Number.isInteger(length) || length < 1 || length > 16)) {
      const error = new Error("Invalid authentication tag length");
      error.code = "ERR_CRYPTO_INVALID_AUTH_TAG";
      throw error;
    }
    if (!chacha && !ocb && (typeof length !== "number" || !Number.isInteger(length) || ![4, 8, 12, 13, 14, 15, 16].includes(length))) {
      const error = new TypeError("Invalid authentication tag length");
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    return { authTagLength: length };
  }
  function parseCipherOptions(options) {
    let authTagLength;
    let ctsMode;
    let xtsStandard;
    if (options !== undefined && options !== null) {
      const value = options.authTagLength;
      if (value != null) {
        if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > 0xffffffff) {
          const error = new TypeError(`The value of "options.authTagLength" is out of range. It must be a non-negative integer. Received ${value}`);
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        authTagLength = value + 0;
      }
      ctsMode = options.ctsMode;
      if (ctsMode != null && typeof ctsMode !== "string") {
        const error = new TypeError(`The "options.ctsMode" argument must be of type string. ${receivedArgument(ctsMode)}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      xtsStandard = options.xtsStandard;
      if (xtsStandard != null && typeof xtsStandard !== "string") {
        const error = new TypeError(`The "options.xtsStandard" argument must be of type string. ${receivedArgument(xtsStandard)}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (ctsMode !== undefined && ctsMode !== null && !["CS1", "CS2", "CS3"].includes(ctsMode)) {
        const error = new TypeError(`The value of "options.ctsMode" is invalid. Received ${JSON.stringify(ctsMode)}`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      if (xtsStandard !== undefined && xtsStandard !== null && !["GB", "IEEE"].includes(xtsStandard)) {
        const error = new TypeError(`The value of "options.xtsStandard" is invalid. Received ${JSON.stringify(xtsStandard)}`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
    }
    if (ctsMode != null || xtsStandard != null) {
      const error = new Error("Cipher options are not supported by this cipher");
      error.code = "ERR_CRYPTO_UNSUPPORTED_OPERATION";
      throw error;
    }
    return { authTagLength };
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
    // Node accepts WebCrypto secret CryptoKeys here for backwards
    // compatibility (DEP0203). The WebCrypto key store owns the canonical
    // bytes, so query it through the shared native key-material bridge rather
    // than asking the guest CryptoKey for an export method.
    const bytesLike = (value) => typeof value === "string" || secretKeys.has(value) ||
      ArrayBuffer.isView(value) || value instanceof ArrayBuffer ||
      typeof SharedArrayBuffer !== "undefined" && value instanceof SharedArrayBuffer;
    const webCryptoKeyBytes = !bytesLike(key) ? secretMaterial(key) : undefined;
    const isWebCryptoKey = webCryptoKeyBytes !== undefined && webCryptoKeyBytes !== key;
    if (!bytesLike(key) && !isWebCryptoKey) {
      const error = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (isWebCryptoKey) {
      process.emitWarning('Passing a CryptoKey to node:crypto functions is deprecated.', 'DeprecationWarning', 'DEP0203');
    }
    const keyBytes = Buffer.from(secretKeys.has(key) ? secretMaterial(secretKeys.get(key)) : isWebCryptoKey ? webCryptoKeyBytes : key);
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
    const ivLength = info.ivLength === undefined && info.mode === "ecb" ? 0 : info.ivLength;
    if (iv === null && ivLength !== 0) throw new Error("Invalid initialization vector");
    const ivBytes = iv === null && ivLength === 0 ? Buffer.alloc(0) : Buffer.from(iv);
    if (info.mode === "gcm" ? ivBytes.length === 0 : info.mode === "ocb" ? ivBytes.length < 1 || ivBytes.length > 15 : ivBytes.length !== ivLength) {
      throw new Error("Invalid initialization vector");
    }
    options = parseCipherOptions(options);
    return new CipherBase(info.name, keyBytes, ivBytes, false, (info.mode === "gcm" || info.mode === "ocb" || info.name === "chacha20-poly1305") ? validateTagLength(options, info.name === "chacha20-poly1305", info.mode === "ocb", info.name) : options);
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
    const webCryptoKeyBytes = !bytesLike(key) ? secretMaterial(key) : undefined;
    const isWebCryptoKey = webCryptoKeyBytes !== undefined && webCryptoKeyBytes !== key;
    if (!bytesLike(key) && !isWebCryptoKey) {
      const error = new TypeError('The "key" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (isWebCryptoKey) {
      process.emitWarning('Passing a CryptoKey to node:crypto functions is deprecated.', 'DeprecationWarning', 'DEP0203');
    }
    const keyBytes = Buffer.from(secretKeys.has(key) ? secretMaterial(secretKeys.get(key)) : isWebCryptoKey ? webCryptoKeyBytes : key);
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
    const ivLength = info.ivLength === undefined && info.mode === "ecb" ? 0 : info.ivLength;
    if (iv === null && ivLength !== 0) throw new Error("Invalid initialization vector");
    const ivBytes = iv === null && ivLength === 0 ? Buffer.alloc(0) : Buffer.from(iv);
    if (info.mode === "gcm" ? ivBytes.length === 0 : info.mode === "ocb" ? ivBytes.length < 1 || ivBytes.length > 15 : ivBytes.length !== ivLength) {
      throw new Error("Invalid initialization vector");
    }
    options = parseCipherOptions(options);
    return new CipherBase(info.name, keyBytes, ivBytes, true, (info.mode === "gcm" || info.mode === "ocb" || info.name === "chacha20-poly1305") ? validateTagLength(options, info.name === "chacha20-poly1305", info.mode === "ocb", info.name) : options);
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
      symmetricKeySize: { get() { return secretMaterial(secretKeys.get(this)).length; }, enumerable: true },
      export: { value() { return Buffer.from(secretMaterial(secretKeys.get(this))); } },
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
    return secretKeys.has(value) ? Buffer.from(secretMaterial(secretKeys.get(value))) : cryptoBytes(value);
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
      if (secretKeys.has(value)) return Buffer.from(secretMaterial(secretKeys.get(value)));
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
  const primeModPow = (base, exponent, modulus) => {
    let result = 1n;
    base %= modulus;
    while (exponent > 0n) {
      if (exponent & 1n) result = result * base % modulus;
      exponent >>= 1n;
      base = base * base % modulus;
    }
    return result;
  };
  const isPrimeValue = (value) => {
    if (value < 2n) return false;
    const bases = [2n, 3n, 5n, 7n, 11n, 13n, 17n, 19n, 23n, 29n, 31n, 37n];
    for (const base of bases) {
      if (value === base) return true;
      if (value % base === 0n) return false;
    }
    let d = value - 1n;
    let s = 0;
    while ((d & 1n) === 0n) { d >>= 1n; s++; }
    for (const base of bases) {
      let x = primeModPow(base % (value - 3n) + 2n, d, value);
      if (x === 1n || x === value - 1n) continue;
      let passed = false;
      for (let round = 1; round < s; round++) {
        x = x * x % value;
        if (x === value - 1n) { passed = true; break; }
      }
      if (!passed) return false;
    }
    return true;
  };
  const unsignedBigInt = (value, name) => {
    if (typeof value === "bigint") {
      if (value < 0n) {
        const error = new RangeError(`The value of "${name}" is out of range. It must be >= 0. Received ${value}n`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      return value;
    }
    if (value === null || !(ArrayBuffer.isView(value) || value instanceof ArrayBuffer)) {
      const error = new TypeError(`The "${name}" argument must be an instance of ArrayBuffer, TypedArray, Buffer, DataView, or bigint. ${receivedArgument(value)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return BigInt(`0x${cryptoBytes(value).toString("hex") || "0"}`);
  };
  const primeInput = (candidate) => {
    if (typeof candidate !== "bigint" && candidate.byteLength > 0x3ffffff) {
      const error = new Error("error: BIGNUM routines: bignum too long");
      error.code = "ERR_OSSL_BN_BIGNUM_TOO_LONG";
      throw error;
    }
    return unsignedBigInt(candidate, "candidate");
  };
  const checkPrimeSync = (candidate, options = {}) => {
    const value = primeInput(candidate);
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const checks = options.checks === undefined ? 0 : options.checks;
    if (typeof checks !== "number") {
      const error = new TypeError(`The "options.checks" property must be of type number. ${receivedArgument(checks)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(checks) || checks < 0 || checks > 0x7fffffff) {
      const error = new RangeError(`The value of "options.checks" is out of range. It must be >= 0 && <= 2147483647. Received ${checks}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    return isPrimeValue(value);
  };
  const checkPrime = (candidate, options, callback) => {
    if (typeof options === "function") { callback = options; options = {}; }
    if (callback === undefined || typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const result = checkPrimeSync(candidate, options === undefined ? {} : options);
    process.nextTick(bindAsyncCallback(callback), null, result);
  };
  const generatePrimeValue = (size, options = {}) => {
    if (typeof size !== "number") {
      const error = new TypeError(`The "size" argument must be of type number. ${receivedArgument(size)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (!Number.isInteger(size) || size < 1 || size > 0x7fffffff) {
      const error = new RangeError(`The value of "size" is out of range. It must be >= 1 && <= 2147483647. Received ${size}`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError(`The "options" argument must be of type object. ${receivedArgument(options)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    for (const name of ["safe", "bigint"]) {
      if (options[name] !== undefined && typeof options[name] !== "boolean") {
        const error = new TypeError(`The "options.${name}" property must be of type boolean. ${receivedArgument(options[name])}`);
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
    }
    const add = options.add === undefined ? undefined : unsignedBigInt(options.add, "options.add");
    const rem = options.rem === undefined ? undefined : unsignedBigInt(options.rem, "options.rem");
    if (size > 8192) throw new RangeError("The requested prime size is too large");
    const limit = (1n << BigInt(size)) - 1n;
    const floor = 1n << BigInt(size - 1);
    for (;;) {
      const bytes = randomBuffer(Math.ceil(size / 8));
      let n = BigInt(`0x${bytes.toString("hex") || "0"}`) & limit;
      n |= floor;
      if (size > 2) n |= 1n << BigInt(size - 2);
      n |= 1n;
      if (add !== undefined) {
        const residue = rem === undefined ? (options.safe ? 3n : 1n) : rem;
        if (rem !== undefined && rem >= add) {
          const error = new RangeError("invalid options.rem"); error.code = "ERR_OUT_OF_RANGE"; throw error;
        }
        if (add >= (1n << BigInt(size))) {
          const error = new RangeError("invalid options.add"); error.code = "ERR_OUT_OF_RANGE"; throw error;
        }
        n += (residue - n % add + add) % add;
      }
      if (n <= limit && n >= floor && isPrimeValue(n) && (!options.safe || isPrimeValue((n - 1n) / 2n))) {
        if (options.bigint) return n;
        let hex = n.toString(16); if (hex.length & 1) hex = `0${hex}`;
        const out = Buffer.alloc(Math.ceil(size / 8)); Buffer.from(hex, "hex").copy(out, Math.max(0, out.length - Math.ceil(hex.length / 2))); return out;
      }
    }
  };
  const generatePrimeSync = (size, options) => generatePrimeValue(size, options === undefined ? {} : options);
  const generatePrime = (size, options, callback) => {
    if (typeof options === "function") { callback = options; options = {}; }
    if (typeof size !== "number") {
      const error = new TypeError(`The "size" argument must be of type number. ${receivedArgument(size)}`);
      error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    if (!Number.isInteger(size) || size < 1 || size > 0x7fffffff) {
      const error = new RangeError(`The value of "size" is out of range. It must be >= 1 && <= 2147483647. Received ${size}`);
      error.code = "ERR_OUT_OF_RANGE"; throw error;
    }
    if (options === null || options === undefined) options = {};
    if (typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError(`The "options" argument must be of type object. ${receivedArgument(options)}`);
      error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    for (const name of ["safe", "bigint"]) {
      if (options[name] !== undefined && typeof options[name] !== "boolean") {
        const error = new TypeError(`The "options.${name}" property must be of type boolean. ${receivedArgument(options[name])}`);
        error.code = "ERR_INVALID_ARG_TYPE"; throw error;
      }
    }
    for (const name of ["add", "rem"]) {
      const value = options[name];
      if (value !== undefined && typeof value !== "bigint" && !(ArrayBuffer.isView(value) || value instanceof ArrayBuffer)) {
        const error = new TypeError(`The "options.${name}" argument must be an instance of ArrayBuffer, TypedArray, Buffer, DataView, or bigint. ${receivedArgument(value)}`);
        error.code = "ERR_INVALID_ARG_TYPE"; throw error;
      }
      if (typeof value === "bigint" && value < 0n) unsignedBigInt(value, `options.${name}`);
    }
    if (callback === undefined || typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function'); error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    setImmediate(() => { try { callback(null, generatePrimeValue(size, options === undefined ? {} : options)); } catch (error) { callback(error); } });
  };
  const modp2 = BigInt("0xFFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF");
  const dhBytesToInt = (value, encoding) => {
    const bytes = typeof value === "string" ? Buffer.from(value, encoding || "utf8") : cryptoBytes(value);
    let result = 0n;
    for (const byte of bytes) result = (result << 8n) | BigInt(byte);
    return result;
  };
  const dhIntToBuffer = (value, length) => {
    let hex = value.toString(16); if (hex.length & 1) hex = `0${hex}`;
    const raw = Buffer.from(hex, "hex");
    if (length === undefined) return raw;
    const out = Buffer.alloc(length); raw.copy(out, Math.max(0, length - raw.length)); return out;
  };
  const dhModPow = (base, exponent, modulus) => {
    let result = 1n; base %= modulus;
    while (exponent > 0n) { if (exponent & 1n) result = result * base % modulus; exponent >>= 1n; base = base * base % modulus; }
    return result;
  };
  const dhEncoding = (bytes, encoding) => encoding === undefined || encoding === "buffer" ? bytes : bytes.toString(encoding);
  class DiffieHellmanImpl {
    constructor(sizeOrKey, keyEncoding, generator, generatorEncoding) {
      let prime;
      if (typeof sizeOrKey === "number") {
        if (!Number.isInteger(sizeOrKey)) {
          const error = new RangeError(`The value of "sizeOrKey" is out of range. It must be an integer. Received ${sizeOrKey}`);
          error.code = "ERR_OUT_OF_RANGE"; throw error;
        }
        if (sizeOrKey <= 1) {
          const error = new Error("error:0280007E:Diffie-Hellman routines::modulus too small");
          error.code = "ERR_OSSL_DH_MODULUS_TOO_SMALL"; throw error;
        }
        prime = sizeOrKey <= 1024 ? modp2 : generatePrimeValue(sizeOrKey, { safe: true, bigint: true });
        if (keyEncoding !== undefined) generator = typeof keyEncoding === "number" ? BigInt(keyEncoding || 2) : dhBytesToInt(keyEncoding);
      } else {
        if (typeof sizeOrKey !== "string" && !(ArrayBuffer.isView(sizeOrKey) || sizeOrKey instanceof ArrayBuffer)) {
          const error = new TypeError(`The "sizeOrKey" argument must be of type number or string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView. ${receivedArgument(sizeOrKey)}`);
          error.code = "ERR_INVALID_ARG_TYPE"; throw error;
        }
        if (typeof keyEncoding === "string" && !["utf8", "utf-8", "hex", "base64", "base64url", "latin1", "binary", "ascii", "ucs2", "ucs-2", "utf16le", "utf-16le", "buffer"].includes(keyEncoding.toLowerCase())) {
          generatorEncoding = generator;
          generator = keyEncoding;
          keyEncoding = undefined;
        }
        if (keyEncoding !== undefined && typeof keyEncoding !== "string") {
          generator = keyEncoding;
          keyEncoding = undefined;
        }
        const enc = typeof keyEncoding === "string" ? keyEncoding : "utf8";
        prime = dhBytesToInt(sizeOrKey, enc);
        if (generator === undefined) generator = 2n;
        else if (typeof generator === "number") {
          if (!Number.isInteger(generator)) {
            const error = new RangeError(`The value of "generator" is out of range. It must be an integer. Received ${generator}`);
            error.code = "ERR_OUT_OF_RANGE"; throw error;
          }
          generator = BigInt(generator || 2);
        } else if (typeof generator === "string" || ArrayBuffer.isView(generator) || generator instanceof ArrayBuffer) {
          generator = dhBytesToInt(generator, generatorEncoding);
        } else {
          const error = new TypeError(`The "generator" argument must be of type number or string or an instance of ArrayBuffer, Buffer, TypedArray, or DataView. ${receivedArgument(generator)}`);
          error.code = "ERR_INVALID_ARG_TYPE"; throw error;
        }
      }
      this._p = prime;
      this._g = typeof generator === "bigint" ? generator : BigInt(generator || 2);
      if (this._g <= 1n || (this._p > 3n && this._g >= this._p - 1n)) {
        const error = new Error("error:02800075:Diffie-Hellman routines::bad generator");
        error.code = "ERR_OSSL_DH_BAD_GENERATOR"; throw error;
      }
      this._private = undefined;
      this._public = undefined;
      this._publicOutdated = false;
      this.verifyError = 0;
      if (!isPrimeValue(this._p)) this.verifyError |= 1;
      else if (this._p > 3n && !isPrimeValue((this._p - 1n) / 2n)) this.verifyError |= 2;
      if (this._p.toString(2).length < 512) this.verifyError |= 4;
      if (this._g < 2n || this._g > this._p - 2n) this.verifyError |= 8;
    }
    generateKeys(encoding) {
      if (this._private === undefined) {
        const bits = this._p.toString(2).length;
        this._private = dhBytesToInt(randomBuffer(Math.ceil(bits / 8))) % (this._p - 3n) + 2n;
      }
      if (this._public === undefined || this._publicOutdated) {
        this._public = dhModPow(this._g, this._private, this._p);
        this._publicOutdated = false;
      }
      return dhEncoding(dhIntToBuffer(this._public), encoding);
    }
    computeSecret(other, inputEncoding, outputEncoding) {
      if (this._private === undefined) {
        const error = new Error("Cannot compute shared secret without a private key");
        error.code = "ERR_CRYPTO_INVALID_STATE"; throw error;
      }
      const value = dhBytesToInt(other, inputEncoding);
      if (value <= 1n || value >= this._p - 1n) {
        const error = new Error(value <= 1n ? "Supplied key is too small" : "Supplied key is too large"); error.code = "ERR_CRYPTO_INVALID_KEYLEN"; throw error;
      }
      const secret = dhModPow(value, this._private, this._p);
      // OpenSSL's DH_compute_key_padded returns the shared secret at the
      // prime's byte width, including leading zero bytes.  Keeping that
      // padding is observable for fixed-width DH protocols and matches
      // Node's DiffieHellman.computeSecret().
      const bytes = dhIntToBuffer(secret, Math.ceil(this._p.toString(2).length / 8));
      return dhEncoding(bytes, outputEncoding);
    }
    getPrime(encoding) { return dhEncoding(dhIntToBuffer(this._p), encoding); }
    getGenerator(encoding) { return dhEncoding(dhIntToBuffer(this._g), encoding); }
    getPublicKey(encoding) {
      if (this._public === undefined) { const error = new Error("No public key - did you forget to generate one?"); error.code = "ERR_CRYPTO_INVALID_STATE"; throw error; }
      return dhEncoding(dhIntToBuffer(this._public), encoding);
    }
    getPrivateKey(encoding) {
      if (this._private === undefined) { const error = new Error("No private key - did you forget to generate one?"); error.code = "ERR_CRYPTO_INVALID_STATE"; throw error; }
      return dhEncoding(dhIntToBuffer(this._private), encoding);
    }
    setPublicKey(key, encoding) { this._public = dhBytesToInt(key, encoding); return dhEncoding(dhIntToBuffer(this._public), encoding); }
    setPrivateKey(key, encoding) { this._private = dhBytesToInt(key, encoding); this._publicOutdated = this._public !== undefined; return dhEncoding(dhIntToBuffer(this._private), encoding); }
  }
  function DiffieHellman(sizeOrKey, keyEncoding, generator, generatorEncoding) {
    return new DiffieHellmanImpl(sizeOrKey, keyEncoding, generator, generatorEncoding);
  }
  DiffieHellman.prototype = DiffieHellmanImpl.prototype;
  DiffieHellman.prototype.constructor = DiffieHellman;
  const createDiffieHellman = (sizeOrKey, keyEncoding, generator, generatorEncoding) =>
    new DiffieHellman(sizeOrKey, keyEncoding, generator, generatorEncoding);
  const getDiffieHellman = (name) => {
    if (typeof name !== "string") { const error = new TypeError('The "name" argument must be of type string'); error.code = "ERR_INVALID_ARG_TYPE"; throw error; }
    if (!/^modp([125]|14)$/i.test(name)) { const error = new Error("Unknown DH group"); error.code = "ERR_CRYPTO_UNKNOWN_DH_GROUP"; throw error; }
    return new DiffieHellmanGroup(name);
  };
  function DiffieHellmanGroup(name) {
    const primes = {
      modp1: "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A63A3620FFFFFFFFFFFFFFFF",
      modp2: "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7EDEE386BFB5A899FA5AE9F24117C4B1FE649286651ECE65381FFFFFFFFFFFFFFFF",
      modp5: "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA237327FFFFFFFFFFFFFFFF".replace(/ /g, ""),
      modp14: "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3BE39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183995497CEA956AE515D2261898FA051015728E5A8AACAA68FFFFFFFFFFFFFFFF".replace(/ /g, ""),
    };
    const prime = primes[name.toLowerCase()];
    const group = new DiffieHellman(Buffer.from(prime, "hex"), "buffer", 2);
    Object.setPrototypeOf(group, DiffieHellmanGroup.prototype);
    return group;
  }
  DiffieHellmanGroup.prototype = Object.create(DiffieHellman.prototype);
  Object.defineProperties(DiffieHellmanGroup.prototype, {
    constructor: { value: DiffieHellmanGroup, writable: true, configurable: true },
    setPrivateKey: { value: undefined, writable: true, configurable: true },
    setPublicKey: { value: undefined, writable: true, configurable: true },
  });
  const createDiffieHellmanGroup = (name) => new DiffieHellmanGroup(name);
  const diffieHellman = (options, callback) => {
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError(`The "options" argument must be of type object. ${receivedArgument(options)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (callback !== undefined && typeof callback !== "function") {
      const error = new TypeError(`The "callback" argument must be of type function. ${receivedArgument(callback)}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const privateInput = options.privateKey;
    const publicInput = options.publicKey;
    if (privateInput == null) {
      const error = new TypeError('The "options.privateKey" property must be of type KeyObject');
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (publicInput == null) {
      const error = new TypeError('The "options.publicKey" property must be of type KeyObject');
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    // Node's key preparation normalizes URLs and encodings into KeyObjects
    // before handing them to the native DH job. Keep the same boundary here.
    let privateKey;
    try {
      privateKey = privateInput instanceof api.KeyObject
        ? privateInput : api.createPrivateKey(privateInput);
    } catch (error) {
      if (error?.code === "ERR_INVALID_ARG_VALUE" && typeof error.message === "string") {
        error.message = error.message.replace(/'key\.(format|type)'/g, "'options.privateKey.$1'");
      }
      throw error;
    }
    let publicKey;
    try {
      publicKey = publicInput instanceof api.KeyObject
        ? publicInput : api.createPublicKey(publicInput);
    } catch (error) {
      if (error?.code === "ERR_INVALID_ARG_VALUE" && typeof error.message === "string") {
        error.message = error.message.replace(/'key\.(format|type)'/g, "'options.publicKey.$1'");
      }
      throw error;
    }
    const run = () => Buffer.from(diffieHellmanNative(privateKey, publicKey));
    if (callback === undefined) return run();
    setImmediate(() => {
      let result;
      try { result = run(); }
      catch (error) { callback(error); return; }
      callback(null, result);
    });
  };
  const ecdhStates = new WeakMap();
  const ecdhCurveSymbol = Symbol("ecdhCurve");
  const ecdhPublicOwners = Object.create(null);
  const ecdhBytes = (value, encoding) => typeof value === "string" ? Buffer.from(value, encoding || "utf8") : cryptoBytes(value);
  const ecdhEncode = (value, encoding) => encoding === undefined || encoding === "buffer" ? Buffer.from(value) : Buffer.from(value).toString(encoding);
  class ECDHImpl {
    constructor(curve) {
      if (typeof curve !== "string") {
        const error = new TypeError(`The "curve" argument must be of type string. ${receivedArgument(curve)}`);
        error.code = "ERR_INVALID_ARG_TYPE"; throw error;
      }
      if (!getCurves().includes(curve.toLowerCase())) {
        const error = new Error(`Invalid curve name: ${curve}`); error.code = "ERR_CRYPTO_INVALID_CURVE"; throw error;
      }
      this.curve = curve;
      ecdhStates.set(this, { privateKey: undefined, publicKey: undefined });
    }
    generateKeys(encoding, format) {
      const state = ecdhStates.get(this);
      if (format !== undefined && !["compressed", "uncompressed", "hybrid"].includes(format)) {
        const error = new TypeError(`Invalid ECDH format: ${String(format)}`); error.code = "ERR_CRYPTO_ECDH_INVALID_FORMAT"; throw error;
      }
      const pair = ecdhNative("generate", this.curve, [], [], format || "uncompressed");
      state.privateKey = Buffer.from(pair[0]); state.publicKey = Buffer.from(pair[1]);
      const output = ecdhEncode(state.publicKey, encoding);
      if (output && typeof output === "object") Object.defineProperty(output, ecdhCurveSymbol, { value: this.curve.toLowerCase() });
      ecdhPublicOwners[state.publicKey.toString("hex")] = this.curve.toLowerCase();
      return output;
    }
    computeSecret(other, inputEncoding, outputEncoding) {
      if (other && typeof other === "object" && other[ecdhCurveSymbol] !== undefined && other[ecdhCurveSymbol] !== this.curve.toLowerCase()) {
        const error = new Error("Public key is not valid for specified curve"); error.code = "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY"; throw error;
      }
      const state = ecdhStates.get(this);
      if (!state || !state.privateKey) { const error = new Error("ECDH key is not initialized"); error.code = "ERR_CRYPTO_INVALID_STATE"; throw error; }
      const peerBytes = ecdhBytes(other, inputEncoding);
      const owner = ecdhPublicOwners[Buffer.from(peerBytes).toString("hex")];
      if (owner !== undefined && owner !== this.curve.toLowerCase()) {
        const error = new Error("Public key is not valid for specified curve"); error.code = "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY"; throw error;
      }
      const currentPair = ecdhNative("private", this.curve, state.privateKey, [], "uncompressed");
      if (!currentPair || currentPair[1] === undefined || !state.publicKey) { const error = new Error("ECDH key state is invalid"); error.code = "ERR_CRYPTO_INVALID_STATE"; throw error; }
      if (Buffer.from(currentPair[1]).toString("hex") !== state.publicKey.toString("hex")) throw new Error("Invalid key pair");
      try { return ecdhEncode(ecdhNative("secret", this.curve, state.privateKey, peerBytes, "uncompressed"), outputEncoding); }
      catch (_) { const error = new Error("Public key is not valid for specified curve"); error.code = "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY"; throw error; }
    }
    getPrivateKey(encoding) {
      const state = ecdhStates.get(this);
      if (!state.privateKey) { const error = new Error("Failed to get ECDH private key"); error.code = "ERR_CRYPTO_INVALID_STATE"; throw error; }
      return ecdhEncode(state.privateKey, encoding);
    }
    getPublicKey(encoding, format) {
      const state = ecdhStates.get(this);
      if (format !== undefined && !["compressed", "uncompressed", "hybrid"].includes(format)) {
        const error = new TypeError(`Invalid ECDH format: ${String(format)}`); error.code = "ERR_CRYPTO_ECDH_INVALID_FORMAT"; throw error;
      }
      if (!state.publicKey) { const error = new Error("Failed to get ECDH public key"); error.code = "ERR_CRYPTO_INVALID_STATE"; throw error; }
      const output = ecdhEncode(ecdhNative("format", this.curve, [], state.publicKey, format || "uncompressed"), encoding);
      if (output && typeof output === "object") Object.defineProperty(output, ecdhCurveSymbol, { value: this.curve.toLowerCase() });
      ecdhPublicOwners[Buffer.from(output).toString("hex")] = this.curve.toLowerCase();
      return output;
    }
    setPrivateKey(key, encoding) {
      const state = ecdhStates.get(this);
      let pair;
      try { pair = ecdhNative("private", this.curve, ecdhBytes(key, encoding), [], "uncompressed"); }
      catch (_) { throw new Error("Private key is not valid for specified curve"); }
      state.privateKey = Buffer.from(pair[0]); state.publicKey = Buffer.from(pair[1]);
      return ecdhEncode(state.privateKey, encoding);
    }
    setPublicKey(key, encoding) {
      const state = ecdhStates.get(this);
      process.emitWarning("ecdh.setPublicKey() is deprecated.", { type: "DeprecationWarning", code: "DEP0031" });
      try { state.publicKey = Buffer.from(ecdhNative("public", this.curve, [], ecdhBytes(key, encoding), "uncompressed")); }
      catch (_) { const error = new Error("Failed to convert Buffer to EC_POINT"); error.code = "ERR_CRYPTO_ECDH_INVALID_PUBLIC_KEY"; throw error; }
      return ecdhEncode(state.publicKey, encoding);
    }
  }
  function ECDH(curve) { return new ECDHImpl(curve); }
  ECDH.prototype = ECDHImpl.prototype;
  ECDH.prototype.constructor = ECDH;
  ECDH.convertKey = (key, curve, inputEncoding, outputEncoding, format = "uncompressed") => {
    if (key === undefined || key === null) {
      const error = new TypeError('The "key" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView'); error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    if (typeof curve !== "string") {
      const error = new TypeError('The "curve" argument must be of type string'); error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    if (!getCurves().includes(curve.toLowerCase())) throw new TypeError("Invalid EC curve name");
    if (!["compressed", "uncompressed", "hybrid"].includes(format)) {
      const error = new TypeError(`Invalid ECDH format: ${String(format)}`); error.code = "ERR_CRYPTO_ECDH_INVALID_FORMAT"; throw error;
    }
    try {
      const result = ecdhNative("format", curve, [], ecdhBytes(key, inputEncoding), format);
      return ecdhEncode(result, outputEncoding);
    } catch (_) {
      throw new Error("Failed to convert Buffer to EC_POINT");
    }
  };
  const createECDH = (curve) => new ECDH(curve);
  const rsaCrypt = (decrypt, privateKey, key, data) => {
    const material = key && typeof key === "object" && key.key !== undefined ? key.key : key;
    const keyBytes = typeof material === "string" ? Buffer.from(material) : cryptoBytes(material);
    const passphrase = key && typeof key === "object" && key.passphrase !== undefined ? cryptoBytes(key.passphrase) : Buffer.alloc(0);
    const format = key && typeof key === "object" && key.format || "pem";
    const type = key && typeof key === "object" && key.type || "spki";
    const padding = key && typeof key === "object" && key.padding !== undefined ? key.padding : 1;
    return Buffer.from(rsaCryptNative(decrypt, privateKey, keyBytes, cryptoBytes(data), passphrase, format, type, padding));
  };
  const publicEncrypt = (key, data) => rsaCrypt(false, false, key, data);
  const privateDecrypt = (key, data) => rsaCrypt(true, true, key, data);
  const privateEncrypt = (key, data) => rsaCrypt(false, true, key, data);
  const publicDecrypt = (key, data) => rsaCrypt(true, false, key, data);
  const keyObjectBrand = new WeakMap();
  captureKeyBrands(secretKeys, keyObjectBrand);
  class KeyObject {
    constructor(type, pem, der, asymmetricKeyType, pkcs1Pem, pkcs1Der, asymmetricKeyDetails) {
      this.type = type;
      this.asymmetricKeyType = asymmetricKeyType;
      if (asymmetricKeyDetails !== undefined) this.asymmetricKeyDetails = asymmetricKeyDetails;
      this.key = Buffer.from(pem);
      keyObjectBrand.set(this, { type, pem: Buffer.from(pem), der: Buffer.from(der), pkcs1Pem: Buffer.from(pkcs1Pem || pem), pkcs1Der: Buffer.from(pkcs1Der || der), asymmetricKeyType });
    }
    export(options = {}) {
      const state = keyObjectBrand.get(this);
      if (options.format === "der") {
        const output = Buffer.from(options.type === "pkcs1" ? state.pkcs1Der : state.der);
        return options.encoding ? output.toString(options.encoding) : output;
      }
      const output = options.type === "pkcs1" ? Buffer.from(state.pkcs1Pem) : Buffer.from(state.pem);
      return options.encoding ? output.toString(options.encoding) : output;
    }
  }
  const createPrivateKey = (value) => {
    const material = value && typeof value === "object" && value.key !== undefined ? value.key : value;
    const pem = typeof material === "string" ? Buffer.from(material) : cryptoBytes(material);
    return new KeyObject("private", pem, pem, "rsa");
  };
  const createPublicKey = (value) => {
    const material = value && typeof value === "object" && value.key !== undefined ? value.key : value;
    const pem = typeof material === "string" ? Buffer.from(material) : cryptoBytes(material);
    return new KeyObject("public", pem, pem, "rsa");
  };
  const x509States = new WeakMap();
  class X509Certificate {
    constructor(value) {
      if (typeof value === "string") value = Buffer.from(value);
      if (!ArrayBuffer.isView(value)) {
        const error = new TypeError('The "buffer" argument must be of type string or an instance of Buffer, TypedArray, or DataView');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      x509States.set(this, x509Metadata(value));
    }
    get subject() { return x509States.get(this).subject; }
    get issuer() { return x509States.get(this).issuer; }
    get subjectAltName() { return x509States.get(this).subjectAltName; }
    get infoAccess() { return x509States.get(this).infoAccess; }
    checkHost(name, options = {}) { const state = x509States.get(this); return x509CheckHost(state.raw, name, options); }
    get validFrom() { return x509States.get(this).validFrom; }
    get validTo() { return x509States.get(this).validTo; }
    get validFromDate() { return new Date(this.validFrom); }
    get validToDate() { return new Date(this.validTo); }
    get serialNumber() { return x509States.get(this).serialNumber; }
    get fingerprint() { return x509States.get(this).fingerprint; }
    get fingerprint256() { return x509States.get(this).fingerprint256; }
    get fingerprint512() { return x509States.get(this).fingerprint512; }
    get pem() { return x509States.get(this).pem; }
    get raw() { return x509States.get(this).raw; }
    get publicKey() { return x509States.get(this).publicKey; }
    checkCA() { return x509States.get(this).ca; }
    toLegacyObject() { return x509States.get(this).legacy; }
    toString() { return this.pem; }
    toJSON() { return this.pem; }
  }
  const keyOutput = (key, encoding, fallback) => {
    if (!encoding) return key;
    const data = key.export({ format: encoding.format || "pem", type: encoding.type || fallback, encoding: encoding.format === "der" ? undefined : encoding.encoding });
    if (encoding.format !== "der" && encoding.encoding) return data.toString(encoding.encoding);
    return encoding.format === "pem" || encoding.format === undefined ? data.toString("utf8") : data;
  };
  const generateKeyPairSync = (type, options = {}) => {
    if (typeof type !== "string") {
      const error = new TypeError(`The "type" argument must be of type string. ${receivedArgument(type)}`);
      error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    if (options === null || typeof options !== "object" || Array.isArray(options)) {
      const error = new TypeError(`The "options" argument must be of type object. ${receivedArgument(options)}`);
      error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    if (type !== "rsa" && type !== "rsa-pss") {
      const error = new Error(`Key generation for ${type} keys is not supported`);
      error.code = "ERR_CRYPTO_OPERATION_FAILED"; throw error;
    }
    const encodings = [options.publicKeyEncoding, options.privateKeyEncoding];
    if (encodings.some((encoding) => encoding &&
        ["raw-public", "raw-private", "raw-seed"].includes(encoding.format))) {
      const error = new Error("The selected key encoding is incompatible with the key type");
      error.code = "ERR_CRYPTO_INCOMPATIBLE_KEY_OPTIONS";
      throw error;
    }
    const bits = options.modulusLength === undefined ? 2048 : options.modulusLength;
    const exponent = options.publicExponent === undefined ? 65537 : options.publicExponent;
    if (!Number.isInteger(bits) || bits < 512 || bits > 16384) throw rangeError("options.modulusLength", bits);
    if (!Number.isSafeInteger(exponent) || exponent < 3 || exponent > 0xffffffff) throw rangeError("options.publicExponent", exponent);
    const privateEncoding = options.privateKeyEncoding || {};
    const encryptedPassphrase = privateEncoding.passphrase === undefined
      ? Buffer.alloc(0)
      : typeof privateEncoding.passphrase === "string"
        ? Buffer.from(privateEncoding.passphrase)
        : cryptoBytes(privateEncoding.passphrase);
    const result = generateRsaKeyPairNative(bits, exponent, privateEncoding.cipher || "", encryptedPassphrase);
    const privatePem = Buffer.from(result[0]);
    const publicPem = Buffer.from(result[1]);
    const privateDer = Buffer.from(result[2]);
    const publicDer = Buffer.from(result[3]);
    const publicPkcs1 = Buffer.from(result[4]);
    const privatePkcs1 = Buffer.from(result[5]);
    const publicPkcs1Der = Buffer.from(result[6]);
    const privatePkcs1Der = Buffer.from(result[7]);
    const encryptedPrivatePem = Buffer.from(result[8]);
    const keyDetails = { modulusLength: bits, publicExponent: BigInt(exponent) };
    const privateKey = new KeyObject("private", privatePem, privateDer, type, privatePkcs1, privatePkcs1Der, keyDetails);
    const publicKey = new KeyObject("public", publicPem, publicDer, type, publicPkcs1, publicPkcs1Der, keyDetails);
    return {
      publicKey: keyOutput(publicKey, options.publicKeyEncoding, "spki"),
      privateKey: privateEncoding.cipher
        ? encryptedPrivatePem.toString(privateEncoding.encoding || "utf8")
        : keyOutput(privateKey, options.privateKeyEncoding, "pkcs8"),
    };
  };
  const generateKeyPair = (type, options, callback) => {
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE"; throw error;
    }
    // Node rejects an unknown private-key cipher synchronously, before it
    // schedules key generation or invokes the callback. Keep this option
    // validation outside the async-operation error path below.
    const cipher = options?.privateKeyEncoding?.cipher;
    if (cipher && getCipherInfo(cipher) === undefined) {
      const error = new Error("Unknown cipher");
      error.code = "ERR_CRYPTO_UNKNOWN_CIPHER";
      throw error;
    }
    try {
      const result = generateKeyPairSync(type, options);
      process.nextTick(bindAsyncCallback(callback), null, result.publicKey, result.privateKey);
    } catch (error) { process.nextTick(bindAsyncCallback(callback), error); }
  };
  const webCrypto = getWebCrypto ? null : {};
  if (!getWebCrypto) webCrypto.getRandomValues = function(values) {
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
    timingSafeEqual,
    hash: hashOnce,
    createHmac: (algorithm, key) => new Hmac(algorithm, key),
    setEngine,
    createSign: (algorithm) => new Sign(algorithm),
    createSecretKey,
    KeyObject,
    X509Certificate,
    createPrivateKey,
    createPublicKey,
    createCipheriv,
    createDecipheriv,
    Cipheriv,
    Decipheriv,
    getCipherInfo,
    getCiphers,
    getCurves,
    getMacs: () => typeof getMacsNative === "function" ? getMacsNative() : ["hmac"],
    createMac(algorithm, key, options) {
      if (typeof algorithm !== "string") {
        const error = new TypeError('The "algorithm" argument must be of type string');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (algorithm.length === 0 || algorithm.includes("\0")) {
        const error = new TypeError(`The argument 'algorithm' must be non-empty and contain no NUL bytes. Received '${algorithm}'`);
        error.code = "ERR_INVALID_ARG_VALUE";
        throw error;
      }
      const name = algorithm.toLowerCase();
      if (options !== undefined && (options === null || typeof options !== "object" || Array.isArray(options))) {
        const error = new TypeError('The "options" argument must be of type object');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      options ||= {};
      for (const field of ["digest", "cipher"]) {
        if (options[field] !== undefined && typeof options[field] !== "string") {
          const error = new TypeError(`The "options.${field}" property must be of type string`);
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
      }
      for (const field of ["iv", "customization", "salt"]) {
        const value = options[field];
        if (value !== undefined && !(ArrayBuffer.isView(value) || value instanceof ArrayBuffer)) {
          const error = new TypeError(`The "options.${field}" property must be an ArrayBuffer or an ArrayBuffer view`);
          error.code = "ERR_INVALID_ARG_TYPE";
          throw error;
        }
      }
      if (options.outputLength !== undefined && typeof options.outputLength !== "number") {
        const error = new TypeError('The "options.outputLength" property must be of type number');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (options.outputLength !== undefined &&
          (!Number.isInteger(options.outputLength) || options.outputLength < 0 || options.outputLength > 0xffffffff)) {
        const detail = !Number.isInteger(options.outputLength)
          ? ` It must be an integer. Received ${options.outputLength}`
          : ` It must be >= 0 && <= 4294967295. Received ${options.outputLength}`;
        const error = new RangeError(`The value of "options.outputLength" is out of range.${detail}`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
      // Validate the generic options before checking the provider inventory.
      // EVP_MAC algorithms share these option types even when a particular
      // algorithm is unavailable, and Node reports malformed options first.
      const supportedMacs = typeof getMacsNative === "function" ? getMacsNative() : ["hmac"];
      if (!supportedMacs.includes(name)) {
        const error = new TypeError(`Invalid MAC: ${algorithm}`);
        error.code = "ERR_CRYPTO_INVALID_MAC";
        throw error;
      }
      if (!(ArrayBuffer.isView(key) ||
            key instanceof ArrayBuffer || secretKeys.has(key))) {
        const error = new TypeError('The "key" argument must be an instance of ArrayBuffer, TypedArray, DataView, Buffer, or KeyObject.');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (name === "hmac") {
        if (options.digest === undefined) {
          const error = new TypeError("The property 'options.digest' is required for HMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (options.digest.length === 0 || options.digest.includes("\0")) {
          const error = new TypeError("The argument 'options.digest' must be non-empty and contain no NUL bytes");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (["cipher", "iv", "customization", "salt", "outputLength"].some((key) => options[key] !== undefined)) {
          const error = new TypeError("Invalid options for HMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
      } else if (name === "cmac") {
        if (options.cipher === undefined) {
          const error = new TypeError("The property 'options.cipher' is required for CMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (options.cipher.length === 0 || options.cipher.includes("\0")) {
          const error = new TypeError("The argument 'options.cipher' must be non-empty and contain no NUL bytes");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (["digest", "iv", "customization", "salt", "outputLength"].some((key) => options[key] !== undefined)) {
          const error = new TypeError("Invalid options for CMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
      } else if (name === "gmac") {
        if (options.cipher === undefined) {
          const error = new TypeError("The property 'options.cipher' is required for GMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (options.iv === undefined) {
          const error = new TypeError("The property 'options.iv' is required for GMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (options.cipher.length === 0 || options.cipher.includes("\0")) {
          const error = new TypeError("The argument 'options.cipher' must be non-empty and contain no NUL bytes");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
        if (["digest", "customization", "salt", "outputLength"].some((key) => options[key] !== undefined)) {
          const error = new TypeError("Invalid options for GMAC");
          error.code = "ERR_INVALID_ARG_VALUE";
          throw error;
        }
      } else if (name === "poly1305") {
        for (const field of ["digest", "cipher", "iv", "customization", "salt", "outputLength"]) {
          if (options[field] !== undefined) {
            const error = new TypeError(`The property 'options.${field}' is not supported by MAC poly1305`);
            error.code = "ERR_INVALID_ARG_VALUE";
            throw error;
          }
        }
      } else if (name === "siphash") {
        // OpenSSL's SipHash provider requires an exact 16-byte key. Its
        // initialization failure is a generic crypto operation error rather
        // than a JS argument TypeError, and does not expose the OpenSSL stack.
        const keyBytes = secretKeys.has(key)
          ? Buffer.from(secretMaterial(secretKeys.get(key)))
          : keyBuffer(key);
        if (keyBytes.length !== 16) {
          const error = new Error("Failed to initialize MAC");
          error.code = "ERR_CRYPTO_OPERATION_FAILED";
          throw error;
        }
      } else {
        const error = new TypeError(`MAC algorithm ${name} requires provider-specific options that are not supported here`);
        error.code = "ERR_CRYPTO_INVALID_MAC";
        throw error;
      }
      return new Mac(name, key, options);
    },
    getFips: () => 0,
    getHashes,
    randomBytes: randomBuffer,
    DiffieHellman,
    DiffieHellmanGroup,
    createDiffieHellman,
    createDiffieHellmanGroup,
    ECDH,
    createECDH,
    publicEncrypt,
    privateDecrypt,
    privateEncrypt,
    publicDecrypt,
    getDiffieHellman,
    diffieHellman,
    generateKeyPair,
    generateKeyPairSync,
    checkPrime,
    checkPrimeSync,
    generatePrime,
    generatePrimeSync,
    argon2: argon2Native,
    argon2Sync: argon2SyncNative,
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
  Object.defineProperty(api,"constants",{value:nativeConstants,enumerable:true});
  Object.defineProperties(api, {
    webcrypto: { enumerable: true, get() { return getWebCrypto ? getWebCrypto() : webCrypto; } },
    pseudoRandomBytes: { configurable: true, writable: true, value: pseudoRandomBuffer },
    prng: { configurable: true, value: randomBuffer },
    rng: { configurable: true, value: randomBuffer },
  });

  if (getWebCrypto) Object.defineProperties(api, {
    subtle: { enumerable: true, get() { return getWebCrypto().subtle; } },
    getRandomValues: { enumerable: true, get() { return randomValuesSource; } },
  });
  return api;
}"#
);

/// Node exposes crypto before imports, but initializes its implementation lazily.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let global = context.global_root()?;

    #[cfg(quench_openssl_provider)]
    {
        let exceptions = context.host_function(crate::host::shared_vm::operation(
            "webDomExceptionInitialize",
        ))?;
        context.define_lazy_data_properties_rooted(
            global,
            &["DOMException", "QuotaExceededError"],
            false,
            exceptions,
        )?;
        let initialize =
            context.host_function(crate::host::shared_vm::operation("cryptoWebClasses"))?;
        context.define_lazy_data_properties_rooted(
            global,
            &["Crypto", "SubtleCrypto", "CryptoKey"],
            false,
            initialize,
        )?;
    }
    let getter = context.host_function(crate::host::shared_vm::operation("get crypto"))?;
    let descriptor = context.null_object_rooted()?;
    set(context, descriptor, "get", getter)?;
    let yes = context.boolean(true);
    set(context, descriptor, "enumerable", yes)?;
    set(context, descriptor, "configurable", yes)?;
    let key = context.string_rooted("crypto");
    context.define_property_descriptor_rooted(global, key, descriptor)?;
    Ok(())
}

pub(crate) fn global_crypto(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::crypto_module(context)?;
    get(context, module, "webcrypto")
}

pub(crate) fn module(
    context: &mut NativeContext<'_, NodeHost>,
    transform: RootId,
) -> Result<RootId, RootedError> {
    #[cfg(quench_openssl_provider)]
    let (webcrypto, random_values) = {
        let getter =
            context.host_function(crate::host::shared_vm::operation("cryptoWebNamespace"))?;
        let random = crate::host::shared_vm::commonjs::node_library_export(
            context,
            "crypto.js",
            "internal/crypto/random.js",
            "getRandomValues",
        )?;
        (getter, random)
    };
    #[cfg(not(quench_openssl_provider))]
    let (webcrypto, random_values) = (context.undefined(), context.undefined());
    let factory = context.evaluate_script_rooted(CRYPTO_FACTORY, "node:crypto/shared.js")?;
    let hash = context.host_function(crate::host::shared_vm::operation("cryptoHash"))?;
    let hmac = context.host_function(crate::host::shared_vm::operation("cryptoHmac"))?;
    let mac_digest = context.host_function(crate::host::shared_vm::operation("cryptoMacDigest"))?;
    let sign = context.host_function(crate::host::shared_vm::operation("cryptoSign"))?;
    let verify = context.host_function(crate::host::shared_vm::operation("cryptoVerify"))?;
    let random_bytes =
        context.host_function(crate::host::shared_vm::operation("cryptoRandomBytes"))?;
    let pbkdf2 = context.host_function(crate::host::shared_vm::operation("cryptoPbkdf2"))?;
    let scrypt = context.host_function(crate::host::shared_vm::operation("cryptoScrypt"))?;
    let cipher_process =
        context.host_function(crate::host::shared_vm::operation("cryptoCipherProcess"))?;
    let generate_rsa_key_pair = context.host_function(crate::host::shared_vm::operation(
        "cryptoGenerateRsaKeyPair",
    ))?;
    let rsa_crypt = context.host_function(crate::host::shared_vm::operation("cryptoRsaCrypt"))?;
    let set_engine = context.host_function(crate::host::shared_vm::operation("cryptoSetEngine"))?;
    let secret_material =
        context.host_function(crate::host::shared_vm::operation("cryptoSecretMaterial"))?;
    let native_constants = super::crypto_constants::binding(context)?;
    #[cfg(quench_openssl_provider)]
    let provider_binding = super::tls_shared_vm::facade::binding(context)?;
    #[cfg(quench_openssl_provider)]
    let get_macs_native = get(context, provider_binding, "getMacs")?;
    #[cfg(not(quench_openssl_provider))]
    let get_macs_native = context.undefined();
    #[cfg(quench_openssl_provider)]
    let argon2_module = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "crypto.js",
        "internal/crypto/argon2.js",
    )?;
    #[cfg(quench_openssl_provider)]
    let argon2 = get(context, argon2_module, "argon2")?;
    #[cfg(quench_openssl_provider)]
    let argon2_sync = get(context, argon2_module, "argon2Sync")?;
    #[cfg(quench_openssl_provider)]
    let kem_module = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "crypto.js",
        "internal/crypto/kem.js",
    )?;
    #[cfg(quench_openssl_provider)]
    let kem_encapsulate = get(context, kem_module, "encapsulate")?;
    #[cfg(quench_openssl_provider)]
    let kem_decapsulate = get(context, kem_module, "decapsulate")?;
    #[cfg(not(quench_openssl_provider))]
    let (argon2, argon2_sync) = (context.undefined(), context.undefined());
    let capture_brands =
        context.host_function(crate::host::shared_vm::operation("cryptoCaptureKeyBrands"))?;
    let ecdh = context.host_function(crate::host::shared_vm::operation("cryptoEcdh"))?;
    #[cfg(quench_openssl_provider)]
    let diffie_hellman =
        context.host_function(crate::host::shared_vm::operation("cryptoDiffieHellman"))?;
    #[cfg(not(quench_openssl_provider))]
    let diffie_hellman = context.undefined();
    let handle_symbol =
        context.host_function(crate::host::shared_vm::operation("cryptoHandleSymbol"))?;
    let x509_metadata =
        context.host_function(crate::host::shared_vm::operation("cryptoX509Metadata"))?;
    let x509_check_host =
        context.host_function(crate::host::shared_vm::operation("cryptoX509CheckHost"))?;
    let global = context.global_root()?;
    let buffer = get(context, global, "Buffer")?;
    let undefined = context.undefined();
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            hash,
            hmac,
            mac_digest,
            sign,
            verify,
            buffer,
            random_bytes,
            pbkdf2,
            scrypt,
            transform,
            cipher_process,
            generate_rsa_key_pair,
            rsa_crypt,
            ecdh,
            diffie_hellman,
            set_engine,
            capture_brands,
            handle_symbol,
            webcrypto,
            random_values,
            secret_material,
            native_constants,
            get_macs_native,
            argon2,
            argon2_sync,
            x509_metadata,
            x509_check_host,
        ],
    )?;
    #[cfg(quench_openssl_provider)]
    {
        let get_fips = get(context, provider_binding, "getFipsCrypto")?;
        set(context, module, "getFips", get_fips)?;
        let set_fips = get(context, provider_binding, "setFipsCrypto")?;
        set(context, module, "setFips", set_fips)?;
        set(context, module, "encapsulate", kem_encapsulate)?;
        set(context, module, "decapsulate", kem_decapsulate)?;
        super::crypto_hash_source_shared_vm::install(context, module)?;
        let keygen = crate::host::shared_vm::commonjs::node_library_module(
            context,
            "crypto.js",
            "internal/crypto/keygen.js",
        )?;
        for name in ["generateKey", "generateKeySync"] {
            let function = get(context, keygen, name)?;
            set(context, module, name, function)?;
        }
    }
    Ok(module)
}

/// Parse the X509 fields used by Node's TLS helpers. The certificate remains
/// in OpenSSL's representation until its immutable JavaScript projection is
/// constructed, so Node's PEM and DER inputs share the same parser.
pub(crate) fn x509_metadata(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(bytes) = context.bytes_prefix_rooted(input, usize::MAX) else {
        return Err(type_error(context, "X509 input must be a BufferSource")?);
    };
    let cert = openssl::x509::X509::from_pem(&bytes)
        .or_else(|_| openssl::x509::X509::from_der(&bytes))
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let result = context.object_rooted()?;

    fn assign(
        context: &mut NativeContext<'_, NodeHost>,
        object: RootId,
        name: &str,
        value: RootId,
    ) -> Result<(), RootedError> {
        let key = context.string_rooted(name);
        if context.define_data_property_rooted(object, key, value, true, true, true)? {
            Ok(())
        } else {
            Err(RootedError::host("Cannot define X509 metadata property"))
        }
    }

    for (key, value) in [
        (
            "subject",
            super::tls_shared_vm::certificate::multiline_name(cert.subject_name())
                .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?,
        ),
        (
            "issuer",
            super::tls_shared_vm::certificate::multiline_name(cert.issuer_name())
                .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?,
        ),
        ("validFrom", cert.not_before().to_string()),
        ("validTo", cert.not_after().to_string()),
    ] {
        let value = context.string_rooted(&value);
        assign(context, result, key, value)?;
    }
    let serial = cert
        .serial_number()
        .to_bn()
        .and_then(|value| value.to_hex_str())
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let serial = context.string_rooted(&serial);
    assign(context, result, "serialNumber", serial)?;
    for (key, digest) in [
        ("fingerprint", openssl::hash::MessageDigest::sha1()),
        ("fingerprint256", openssl::hash::MessageDigest::sha256()),
        ("fingerprint512", openssl::hash::MessageDigest::sha512()),
    ] {
        let bytes = cert
            .digest(digest)
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let text = bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":");
        let value = context.string_rooted(&text);
        assign(context, result, key, value)?;
    }
    let der = cert
        .to_der()
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let raw = crate::modules::buffer_shared_vm::from_bytes(context, &der)?;
    assign(context, result, "raw", raw)?;
    let pem = cert
        .to_pem()
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let pem_text = context.string_rooted(&String::from_utf8_lossy(&pem));
    assign(context, result, "pem", pem_text)?;
    let legacy = super::tls_shared_vm::certificate::legacy(context, &cert)?;
    assign(context, result, "legacy", legacy)?;
    let alt_name_key = context.string_rooted("subjectaltname");
    let alt_name = context.get_property_rooted(legacy, alt_name_key)?;
    assign(context, result, "subjectAltName", alt_name)?;
    let info_access_key = context.string_rooted("infoaccess");
    let info_access = context.get_property_rooted(legacy, info_access_key)?;
    assign(context, result, "infoAccess", info_access)?;
    let ca_key = context.string_rooted("ca");
    let ca = context.get_property_rooted(legacy, ca_key)?;
    assign(context, result, "ca", ca)?;
    #[cfg(quench_openssl_provider)]
    {
        let key = cert
            .public_key()
            .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
        let key = super::crypto_key_shared_vm::generated_asymmetric(
            context,
            Arc::new(super::crypto_asymmetric::OwnedKey::Public(key)),
            None,
        )?;
        assign(context, result, "publicKey", key)?;
    }
    #[cfg(not(quench_openssl_provider))]
    {
        let undefined = context.undefined();
        assign(context, result, "publicKey", undefined)?;
    }
    Ok(result)
}

pub(crate) fn x509_check_host(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use foreign_types::ForeignType;
    let bytes = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(bytes) = context.bytes_prefix_rooted(bytes, usize::MAX) else {
        return Err(type_error(context, "X509 input must be a BufferSource")?);
    };
    let cert = openssl::x509::X509::from_pem(&bytes)
        .or_else(|_| openssl::x509::X509::from_der(&bytes))
        .map_err(|error| crate::modules::crypto_error_shared_vm::decorated(context, error))?;
    let hostname = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let Some(hostname) = context.string_text(hostname)? else {
        return Err(type_error(context, "The \"name\" argument must be of type string")?);
    };
    if hostname.is_empty() || hostname.as_bytes().contains(&0) {
        return Ok(context.undefined());
    }
    let options = args.get(2).copied().unwrap_or_else(|| context.undefined());
    let subject = get(context, options, "subject")?;
    let subject = context.string_text(subject)?.unwrap_or_else(|| "default".to_owned());
    let flags = match subject.as_str() {
        "default" => 0,
        "always" => 0x1,
        "never" => 0x20,
        _ => return Err(type_error(context, "Invalid subject option")?),
    };
    let mut peer_name = std::ptr::null_mut();
    let result = unsafe {
        openssl_sys::X509_check_host(
            cert.as_ptr(),
            hostname.as_ptr().cast(),
            hostname.len(),
            flags,
            &mut peer_name,
        )
    };
    if result == 1 {
        let name = if peer_name.is_null() {
            hostname
        } else {
            let value = unsafe { std::ffi::CStr::from_ptr(peer_name) }
                .to_string_lossy()
                .into_owned();
            unsafe { openssl_sys::OPENSSL_free(peer_name.cast()) };
            value
        };
        Ok(context.string_rooted(&name))
    } else {
        if !peer_name.is_null() {
            unsafe { openssl_sys::OPENSSL_free(peer_name.cast()) };
        }
        Ok(context.undefined())
    }
}

fn private_key_from_pem(
    pem: &[u8],
    passphrase: &[u8],
) -> Result<openssl::pkey::PKey<openssl::pkey::Private>, openssl::error::ErrorStack> {
    if passphrase.is_empty() {
        openssl::pkey::PKey::private_key_from_pem(pem).or_else(|_| {
            openssl::rsa::Rsa::private_key_from_pem(pem).and_then(openssl::pkey::PKey::from_rsa)
        })
    } else {
        openssl::pkey::PKey::private_key_from_pem_passphrase(pem, passphrase).or_else(|_| {
            openssl::rsa::Rsa::private_key_from_pem_passphrase(pem, passphrase)
                .and_then(openssl::pkey::PKey::from_rsa)
        })
    }
}

fn private_key_from_data(
    data: &[u8],
    passphrase: &[u8],
    format: &str,
    key_type: &str,
) -> Result<openssl::pkey::PKey<openssl::pkey::Private>, openssl::error::ErrorStack> {
    if format == "der" {
        if key_type == "pkcs1" {
            openssl::rsa::Rsa::private_key_from_der(data).and_then(openssl::pkey::PKey::from_rsa)
        } else {
            openssl::pkey::PKey::private_key_from_der(data)
        }
    } else {
        private_key_from_pem(data, passphrase)
    }
}

fn public_key_from_data(
    data: &[u8],
    format: &str,
    key_type: &str,
) -> Result<openssl::pkey::PKey<openssl::pkey::Public>, openssl::error::ErrorStack> {
    if format == "der" && key_type == "pkcs1" {
        openssl::rsa::Rsa::public_key_from_der_pkcs1(data).and_then(openssl::pkey::PKey::from_rsa)
    } else if format == "der" {
        openssl::pkey::PKey::public_key_from_der(data)
    } else {
        openssl::pkey::PKey::public_key_from_pem(data).or_else(|_| {
            openssl::rsa::Rsa::public_key_from_pem_pkcs1(data)
                .and_then(openssl::pkey::PKey::from_rsa)
        })
    }
}

pub(crate) fn generate_rsa_key_pair(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let bits = args
        .get(0)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|value| {
            value.is_finite() && *value >= 512.0 && *value <= 16384.0 && value.fract() == 0.0
        })
        .ok_or_else(|| RootedError::host("RSA modulus length is invalid"))? as u32;
    let exponent = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|value| {
            value.is_finite() && *value >= 3.0 && *value <= u32::MAX as f64 && value.fract() == 0.0
        })
        .unwrap_or(65537.0) as u32;
    let cipher_name = args
        .get(2)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_default();
    let passphrase = args
        .get(3)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let public_exponent = openssl::bn::BigNum::from_u32(exponent)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let rsa = openssl::rsa::Rsa::generate_with_e(bits, &public_exponent)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let public_pkcs1_pem = rsa
        .public_key_to_pem_pkcs1()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_pkcs1_pem = rsa
        .private_key_to_pem()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let encrypted_private_pem = if cipher_name.is_empty() {
        private_pkcs1_pem.clone()
    } else {
        let cipher = match cipher_name.to_ascii_lowercase().as_str() {
            "aes-128-cbc" => openssl::symm::Cipher::aes_128_cbc(),
            "aes-128-ecb" => openssl::symm::Cipher::aes_128_ecb(),
            "aes-192-cbc" => openssl::symm::Cipher::aes_192_cbc(),
            "aes-256-cbc" => openssl::symm::Cipher::aes_256_cbc(),
            "aes-256-ecb" => openssl::symm::Cipher::aes_256_ecb(),
            _ => return Err(RootedError::host(format!("Unknown cipher: {cipher_name}"))),
        };
        rsa.private_key_to_pem_passphrase(cipher, &passphrase)
            .map_err(|error| RootedError::host(error.to_string()))?
    };
    let public_pkcs1_der = rsa
        .public_key_to_der_pkcs1()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_pkcs1_der = rsa
        .private_key_to_der()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let key =
        openssl::pkey::PKey::from_rsa(rsa).map_err(|error| RootedError::host(error.to_string()))?;
    let private_pem = key
        .private_key_to_pem_pkcs8()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let public_pem = key
        .public_key_to_pem()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_der = key
        .private_key_to_der()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let public_der = key
        .public_key_to_der()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let private_values = private_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_values = public_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_pkcs1_values = public_pkcs1_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private_pkcs1_values = private_pkcs1_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_pkcs1_der_values = public_pkcs1_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private_pkcs1_der_values = private_pkcs1_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let encrypted_private_values = encrypted_private_pem
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private_der_values = private_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let public_der_values = public_der
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    let private = context.array_rooted(&private_values)?;
    let public = context.array_rooted(&public_values)?;
    let public_pkcs1 = context.array_rooted(&public_pkcs1_values)?;
    let private_pkcs1 = context.array_rooted(&private_pkcs1_values)?;
    let public_pkcs1_der = context.array_rooted(&public_pkcs1_der_values)?;
    let private_pkcs1_der = context.array_rooted(&private_pkcs1_der_values)?;
    let encrypted_private = context.array_rooted(&encrypted_private_values)?;
    let private_der = context.array_rooted(&private_der_values)?;
    let public_der = context.array_rooted(&public_der_values)?;
    context.array_rooted(&[
        private,
        public,
        private_der,
        public_der,
        public_pkcs1,
        private_pkcs1,
        public_pkcs1_der,
        private_pkcs1_der,
        encrypted_private,
    ])
}

pub(crate) fn rsa_crypt(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let decrypt = args
        .get(0)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let private = args
        .get(1)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let key_bytes = byte_array(
        context,
        *args
            .get(2)
            .ok_or_else(|| RootedError::host("RSA key is missing"))?,
    )?;
    let input = byte_array(
        context,
        *args
            .get(3)
            .ok_or_else(|| RootedError::host("RSA input is missing"))?,
    )?;
    let passphrase = args
        .get(4)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let format = args
        .get(5)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pem".to_owned());
    let key_type = args
        .get(6)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "spki".to_owned());
    let padding = args
        .get(7)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .unwrap_or(1.0) as i32;
    let padding = match padding {
        1 => openssl::rsa::Padding::PKCS1,
        4 => openssl::rsa::Padding::PKCS1_OAEP,
        _ => return Err(RootedError::host("Unsupported RSA padding")),
    };
    let output = if decrypt {
        let key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut operation = openssl::encrypt::Decrypter::new(&key)
            .map_err(|error| RootedError::host(error.to_string()))?;
        operation
            .set_rsa_padding(padding)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut output = vec![
            0;
            operation
                .decrypt_len(&input)
                .map_err(|error| RootedError::host(error.to_string()))?
        ];
        let length = operation
            .decrypt(&input, &mut output)
            .map_err(|error| RootedError::host(error.to_string()))?;
        output.truncate(length);
        output
    } else if private {
        let key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut operation = openssl::encrypt::Encrypter::new(&key)
            .map_err(|error| RootedError::host(error.to_string()))?;
        operation
            .set_rsa_padding(padding)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut output = vec![
            0;
            operation
                .encrypt_len(&input)
                .map_err(|error| RootedError::host(error.to_string()))?
        ];
        let length = operation
            .encrypt(&input, &mut output)
            .map_err(|error| RootedError::host(error.to_string()))?;
        output.truncate(length);
        output
    } else {
        let key = match public_key_from_data(&key_bytes, &format, &key_type) {
            Ok(key) => key,
            Err(_) => {
                let private_key =
                    private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                let public_pem = private_key
                    .public_key_to_pem()
                    .map_err(|error| RootedError::host(error.to_string()))?;
                openssl::pkey::PKey::public_key_from_pem(&public_pem)
                    .map_err(|error| RootedError::host(error.to_string()))?
            }
        };
        let mut operation = openssl::encrypt::Encrypter::new(&key)
            .map_err(|error| RootedError::host(error.to_string()))?;
        operation
            .set_rsa_padding(padding)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let mut output = vec![
            0;
            operation
                .encrypt_len(&input)
                .map_err(|error| RootedError::host(error.to_string()))?
        ];
        let length = operation
            .encrypt(&input, &mut output)
            .map_err(|error| RootedError::host(error.to_string()))?;
        output.truncate(length);
        output
    };
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn ecdh(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let text_arg = |context: &mut NativeContext<'_, NodeHost>, index: usize, fallback: &str| {
        args.get(index)
            .and_then(|root| context.string_text(*root).ok().flatten())
            .unwrap_or_else(|| fallback.to_owned())
    };
    let action = text_arg(context, 0, "");
    let curve = text_arg(context, 1, "");
    let nid = match curve.to_ascii_lowercase().as_str() {
        "prime256v1" | "secp256r1" => openssl::nid::Nid::X9_62_PRIME256V1,
        "secp256k1" => openssl::nid::Nid::SECP256K1,
        "secp384r1" => openssl::nid::Nid::SECP384R1,
        "secp521r1" => openssl::nid::Nid::SECP521R1,
        "secp224r1" => openssl::nid::Nid::SECP224R1,
        "prime192v1" => openssl::nid::Nid::X9_62_PRIME192V1,
        _ => return Err(RootedError::host(format!("Invalid curve name: {curve}"))),
    };
    let group = openssl::ec::EcGroup::from_curve_name(nid)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let mut ctx =
        openssl::bn::BigNumContext::new().map_err(|error| RootedError::host(error.to_string()))?;
    let format = text_arg(context, 4, "uncompressed");
    let form = match format.as_str() {
        "compressed" => openssl::ec::PointConversionForm::COMPRESSED,
        "hybrid" => openssl::ec::PointConversionForm::HYBRID,
        _ => openssl::ec::PointConversionForm::UNCOMPRESSED,
    };
    let private_bytes = args
        .get(2)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let public_bytes = args
        .get(3)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let point_bytes = |point: &openssl::ec::EcPointRef,
                       group: &openssl::ec::EcGroupRef,
                       ctx: &mut openssl::bn::BigNumContextRef,
                       form| {
        point
            .to_bytes(group, form, ctx)
            .map_err(|error| RootedError::host(error.to_string()))
    };
    let output = match action.as_str() {
        "generate" => {
            let key = openssl::ec::EcKey::generate(&group)
                .map_err(|error| RootedError::host(error.to_string()))?;
            let private = key.private_key().to_vec();
            let public = point_bytes(key.public_key(), &group, &mut ctx, form)?;
            let private = private
                .iter()
                .map(|byte| context.number(f64::from(*byte)))
                .collect::<Vec<_>>();
            let public = public
                .iter()
                .map(|byte| context.number(f64::from(*byte)))
                .collect::<Vec<_>>();
            let private = context.array_rooted(&private)?;
            let public = context.array_rooted(&public)?;
            return context.array_rooted(&[private, public]);
        }
        "private" | "format" | "secret" | "public" => {
            let private = if action != "public" && action != "format" {
                let private = openssl::bn::BigNum::from_slice(&private_bytes)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let mut order = openssl::bn::BigNum::new()
                    .map_err(|error| RootedError::host(error.to_string()))?;
                group
                    .order(&mut order, &mut ctx)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                if private.num_bits() == 0 || private.ucmp(&order) != std::cmp::Ordering::Less {
                    return Err(RootedError::host(
                        "Private key is not valid for specified curve",
                    ));
                }
                Some(private)
            } else {
                None
            };
            let public = if action == "public" || action == "format" || action == "secret" {
                let point = openssl::ec::EcPoint::from_bytes(&group, &public_bytes, &mut ctx)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                if !point
                    .is_on_curve(&group, &mut ctx)
                    .map_err(|error| RootedError::host(error.to_string()))?
                {
                    return Err(RootedError::host(
                        "Public key is not valid for specified curve",
                    ));
                }
                Some(point)
            } else {
                None
            };
            if action == "secret" {
                let private = private.as_ref().expect("private scalar parsed");
                let public = public.as_ref().expect("public point parsed");
                let private_point = {
                    let mut point = openssl::ec::EcPoint::new(&group)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                        .mul_generator2(&group, private, &mut ctx)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                };
                let private_key =
                    openssl::ec::EcKey::from_private_components(&group, private, &private_point)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                let public_key = openssl::ec::EcKey::from_public_key(&group, public)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let private_key = openssl::pkey::PKey::from_ec_key(private_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let public_key = openssl::pkey::PKey::from_ec_key(public_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                let mut deriver = openssl::derive::Deriver::new(&private_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                deriver
                    .set_peer(&public_key)
                    .map_err(|error| RootedError::host(error.to_string()))?;
                deriver
                    .derive_to_vec()
                    .map_err(|error| RootedError::host(error.to_string()))?
            } else {
                let point = if let Some(private) = private.as_ref() {
                    let mut point = openssl::ec::EcPoint::new(&group)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                        .mul_generator2(&group, private, &mut ctx)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    point
                } else {
                    public.expect("public point parsed")
                };
                if action == "format" || action == "public" {
                    point_bytes(&point, &group, &mut ctx, form)?
                } else {
                    let private = private.expect("private scalar parsed");
                    let key = openssl::ec::EcKey::from_private_components(&group, &private, &point)
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    key.check_key()
                        .map_err(|error| RootedError::host(error.to_string()))?;
                    let private = key.private_key().to_vec();
                    let public = point_bytes(key.public_key(), &group, &mut ctx, form)?;
                    let private = private
                        .iter()
                        .map(|byte| context.number(f64::from(*byte)))
                        .collect::<Vec<_>>();
                    let public = public
                        .iter()
                        .map(|byte| context.number(f64::from(*byte)))
                        .collect::<Vec<_>>();
                    let private = context.array_rooted(&private)?;
                    let public = context.array_rooted(&public)?;
                    return context.array_rooted(&[private, public]);
                }
            }
        }
        _ => {
            return Err(RootedError::host(format!(
                "Unknown ECDH operation: {action}"
            )));
        }
    };
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn random_bytes(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(size) = args
        .first()
        .copied()
        .and_then(|size| context.rooted_value(size))
        .and_then(|value| value.as_number())
    else {
        return Err(type_error(
            context,
            "The \"size\" argument must be of type number",
        )?);
    };
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 || size > 16_777_216.0 {
        return Err(type_error(
            context,
            "The \"size\" argument must be a non-negative integer",
        )?);
    }
    let mut bytes = vec![0_u8; size as usize];
    openssl::rand::rand_bytes(&mut bytes)
        .map_err(|_| RootedError::host("crypto random generation failed"))?;
    let values = bytes
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
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
    let iterations = args
        .get(2)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 1.0 && value.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto PBKDF2 iterations are invalid"))?
        as usize;
    let key_length = args
        .get(3)
        .copied()
        .and_then(|root| context.rooted_value(root))
        .and_then(|value| value.as_number())
        .filter(|value| value.is_finite() && *value >= 0.0 && value.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto PBKDF2 key length is invalid"))?
        as usize;
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
    let password = byte_array(
        context,
        *args
            .first()
            .ok_or_else(|| RootedError::host("scrypt password missing"))?,
    )?;
    let salt = byte_array(
        context,
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("scrypt salt missing"))?,
    )?;
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
        "aes-256-ecb" => openssl::symm::Cipher::aes_256_ecb(),
        "aes-128-gcm" => openssl::symm::Cipher::aes_128_gcm(),
        "aes-128-ocb" => openssl::symm::Cipher::aes_128_ocb(),
        "aes-192-gcm" => openssl::symm::Cipher::aes_192_gcm(),
        "aes-192-ocb" => openssl::symm::Cipher::aes_192_ocb(),
        "aes-256-cbc" => openssl::symm::Cipher::aes_256_cbc(),
        "aes-256-gcm" => openssl::symm::Cipher::aes_256_gcm(),
        "aes-256-ocb" => openssl::symm::Cipher::aes_256_ocb(),
        "chacha20-poly1305" => openssl::symm::Cipher::chacha20_poly1305(),
        "des-ede3-cbc" => openssl::symm::Cipher::des_ede3_cbc(),
        _ => {
            return Err(RootedError::host(format!("Unknown cipher: {algorithm}")));
        }
    };
    let key = byte_array(
        context,
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("cipher key is missing"))?,
    )?;
    let iv = byte_array(
        context,
        *args
            .get(2)
            .ok_or_else(|| RootedError::host("cipher IV is missing"))?,
    )?;
    let input = byte_array(
        context,
        *args
            .get(3)
            .ok_or_else(|| RootedError::host("cipher input is missing"))?,
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
    let aad = args
        .get(6)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let auth_tag = args
        .get(7)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let auth_tag_length = args
        .get(8)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .unwrap_or(16.0) as usize;
    let auto_padding = args
        .get(9)
        .and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let is_gcm = algorithm.to_ascii_lowercase().ends_with("-gcm");
    let is_ocb = algorithm.to_ascii_lowercase().ends_with("-ocb");
    let is_aead = is_gcm || is_ocb || algorithm.eq_ignore_ascii_case("chacha20-poly1305");
    let mode = if decrypt {
        openssl::symm::Mode::Decrypt
    } else {
        openssl::symm::Mode::Encrypt
    };
    let iv = if cipher.iv_len() == Some(0) {
        None
    } else {
        Some(iv.as_slice())
    };
    let mut crypter = openssl::symm::Crypter::new(cipher, mode, &key, iv)
        .map_err(|error| RootedError::host(error.to_string()))?;
    crypter.pad(auto_padding);
    // OCB requires its configured tag length (encryption) or tag value
    // (decryption) before processing AAD or payload. GCM accepts its tag at
    // finalization, so retain that existing path below.
    if is_ocb {
        // Node configures the expected tag length during cipher initialization,
        // before a decipher receives the tag value. OpenSSL's provider rejects
        // a shortened tag unless that length is established first.
        crypter
            .set_tag_len(auth_tag_length)
            .map_err(|error| RootedError::host(error.to_string()))?;
        if decrypt && !auth_tag.is_empty() {
            crypter
                .set_tag(&auth_tag)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    if is_aead {
        if !aad.is_empty() {
            crypter
                .aad_update(&aad)
                .map_err(|error| RootedError::host(error.to_string()))?;
        }
    }
    let mut output = vec![0; input.len() + cipher.block_size()];
    let mut written = crypter
        .update(&input, &mut output)
        .map_err(|error| RootedError::host(error.to_string()))?;
    if final_block {
        if is_aead && decrypt && !is_ocb {
            crypter
                .set_tag(&auth_tag)
                .map_err(|error| RootedError::host(error.to_string()))?;
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
        crypter
            .get_tag(&mut tag)
            .map_err(|error| RootedError::host(error.to_string()))?;
        let values = tag
            .iter()
            .map(|byte| context.number(f64::from(*byte)))
            .collect::<Vec<_>>();
        context.array_rooted(&values)?
    } else {
        context.array_rooted(&[])?
    };
    let result = context.object_rooted()?;
    set(context, result, "output", output_array)?;
    set(context, result, "tag", tag_array)?;
    Ok(result)
}

#[cfg(not(quench_openssl_provider))]
pub(crate) fn hash(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto hash algorithm is not a string"))?;
    let Some(input) = args.get(1).copied() else {
        return Err(type_error(
            context,
            "The hash input must be an array of bytes",
        )?);
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
            hasher
                .update(&bytes)
                .map_err(|_| RootedError::host("crypto digest update failed"))?;
            let mut output = vec![0; output_length];
            hasher
                .finish_xof(&mut output)
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

#[cfg(not(quench_openssl_provider))]
pub(crate) fn hmac(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto HMAC algorithm is not a string"))?;
    let key_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC key is missing"))?;
    let input_root = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC input is missing"))?;
    let key = byte_array(context, key_root)?;
    let input = byte_array(context, input_root)?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    // OpenSSL rejects a zero-length HMAC key. A single zero byte has the same
    // block-padded HMAC key representation as an empty key.
    let openssl_key = if key.is_empty() { vec![0] } else { key };
    let key = openssl::pkey::PKey::hmac(&openssl_key).map_err(|error| {
        RootedError::host(format!(
            "crypto HMAC key is invalid ({} bytes): {error}",
            openssl_key.len()
        ))
    })?;
    let mut signer = openssl::sign::Signer::new(digest, &key).map_err(|error| {
        RootedError::host(format!("crypto HMAC initialization failed: {error}"))
    })?;
    signer
        .update(&input)
        .map_err(|_| RootedError::host("crypto HMAC update failed"))?;
    let output = signer
        .sign_to_vec()
        .map_err(|_| RootedError::host("crypto HMAC failed"))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&values)
}

pub(crate) fn verify(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto verification algorithm is not a string"))?;
    let input = byte_array(
        context,
        *args
            .get(1)
            .ok_or_else(|| RootedError::host("crypto verification input is missing"))?,
    )?;
    let key_bytes = byte_array(
        context,
        *args
            .get(2)
            .ok_or_else(|| RootedError::host("crypto public key is missing"))?,
    )?;
    let signature = byte_array(
        context,
        *args
            .get(3)
            .ok_or_else(|| RootedError::host("crypto signature is missing"))?,
    )?;
    let passphrase = args
        .get(4)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let format = args
        .get(5)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pem".to_owned());
    let key_type = args
        .get(6)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "spki".to_owned());
    let key = match public_key_from_data(&key_bytes, &format, &key_type) {
        Ok(key) => key,
        Err(_) => {
            let private_key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type);
            if let Ok(private) = private_key {
                let public = private
                    .public_key_to_pem()
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
    verifier
        .update(&input)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let result = verifier
        .verify(&signature)
        .map_err(|error| RootedError::host(error.to_string()))?;
    Ok(context.boolean(result))
}

pub(crate) fn sign(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto signature algorithm is not a string"))?;
    let input_root = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto signature input is missing"))?;
    let key_root = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("crypto private key is missing"))?;
    let input = byte_array(context, input_root)?;
    let key_bytes = byte_array(context, key_root)?;
    let passphrase = args
        .get(3)
        .copied()
        .map(|root| byte_array(context, root))
        .transpose()?
        .unwrap_or_default();
    let format = args
        .get(4)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pem".to_owned());
    let key_type = args
        .get(5)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "pkcs8".to_owned());
    let key = private_key_from_data(&key_bytes, &passphrase, &format, &key_type)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let digest = openssl::hash::MessageDigest::from_name(&algorithm)
        .ok_or_else(|| RootedError::host("Digest method not supported"))?;
    let mut signer = openssl::sign::Signer::new(digest, &key)
        .map_err(|error| RootedError::host(error.to_string()))?;
    signer
        .update(&input)
        .map_err(|error| RootedError::host(error.to_string()))?;
    let output = signer
        .sign_to_vec()
        .map_err(|error| RootedError::host(error.to_string()))?;
    let values = output
        .iter()
        .map(|byte| context.number(f64::from(*byte)))
        .collect::<Vec<_>>();
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
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .filter(|length| length.is_finite() && *length >= 0.0 && length.fract() == 0.0)
        .ok_or_else(|| RootedError::host("crypto byte input is not array-like"))?
        as usize;
    let mut bytes = Vec::with_capacity(length);
    for index in 0..length {
        let value = get(context, root, &index.to_string())?;
        let byte = context
            .rooted_value(value)
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
        Err(RootedError::host(format!(
            "cannot install crypto property {name}"
        )))
    }
}

fn type_error(
    context: &mut NativeContext<'_, NodeHost>,
    message: &str,
) -> Result<RootedError, RootedError> {
    let error = context.type_error_rooted(message)?;
    Ok(context.throw(error))
}

/// Native hashes use the same private symbol captured by the original Node utility.
/// Resolve on first hash construction, after the crypto module has entered the cache.
pub(crate) fn handle_symbol(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(symbol) = state.borrow().crypto_handle_symbol {
        return Ok(symbol);
    }
    crate::host::shared_vm::commonjs::node_library_export(
        context,
        "crypto.js",
        "internal/crypto/util.js",
        "kHandle",
    )?;
    let symbol = state.borrow().crypto_handle_symbol;
    symbol.ok_or_else(|| RootedError::host("Node crypto handle symbol capture missing"))
}

#[cfg(quench_openssl_provider)]
pub(crate) fn hash(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::crypto_digest_error::{self as errors, Operation};
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto hash algorithm is not a string"))?;
    let input = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto hash input is missing"))?;
    let bytes = byte_array(context, input)?;
    let length = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .filter(|value| !value.is_undefined())
        .map(|value| {
            value
                .as_number()
                .filter(|n| n.is_finite() && *n >= 0.0 && n.fract() == 0.0)
                .map(|n| n as usize)
                .ok_or_else(|| RootedError::host("Invalid digest output length"))
        })
        .transpose()?;
    let digest = super::crypto_digest::Digest::fetch(&algorithm)
        .map_err(|error| errors::throw(context, Operation::Hash, error))?;
    let output = digest
        .hash(&bytes, length)
        .map_err(|error| errors::throw(context, Operation::Hash, error))?;
    let roots = output
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}

pub(crate) fn mac_digest(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let key = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("crypto MAC key is missing"))?;
    let input = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto MAC input is missing"))?;
    let cipher = args
        .get(2)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_default();
    let algorithm = args
        .get(3)
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .unwrap_or_else(|| "cmac".to_string());
    let key = byte_array(context, key)?;
    let input = byte_array(context, input)?;
    #[cfg(quench_openssl_provider)]
    let output = match algorithm.as_str() {
        "cmac" => super::crypto_secret_job_shared_vm::kmac_request::cmac(&key, &cipher, &input)
            .map_err(|error| RootedError::host(format!("CMAC operation failed: {error}")))?,
        "poly1305" => {
            use openssl::pkey::{Id, PKey};
            use openssl::sign::Signer;
            let pkey = PKey::private_key_from_raw_bytes(&key, Id::POLY1305).map_err(|error| {
                RootedError::host(format!("Poly1305 key initialization failed: {error}"))
            })?;
            let mut signer = Signer::new_without_digest(&pkey).map_err(|error| {
                RootedError::host(format!("Poly1305 initialization failed: {error}"))
            })?;
            signer
                .update(&input)
                .map_err(|error| RootedError::host(format!("Poly1305 update failed: {error}")))?;
            signer.sign_to_vec().map_err(|error| {
                RootedError::host(format!("Poly1305 finalization failed: {error}"))
            })?
        }
        _ => return Err(RootedError::host("unsupported crypto MAC algorithm")),
    };
    #[cfg(not(quench_openssl_provider))]
    let output: Vec<u8> = {
        let _ = (key, cipher, input, algorithm);
        return Err(RootedError::host(
            "CMAC is unavailable without OpenSSL provider",
        ));
    };
    let roots = output
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}
#[cfg(quench_openssl_provider)]
pub(crate) fn hmac(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    use super::crypto_digest_error::{self as errors, Operation};
    let algorithm = args
        .first()
        .copied()
        .and_then(|root| context.string_text(root).ok().flatten())
        .ok_or_else(|| RootedError::host("crypto HMAC algorithm is not a string"))?;
    let key = args
        .get(1)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC key is missing"))?;
    let input = args
        .get(2)
        .copied()
        .ok_or_else(|| RootedError::host("crypto HMAC input is missing"))?;
    let key = byte_array(context, key)?;
    let input = byte_array(context, input)?;
    let operation = Operation::Hmac(&algorithm);
    let digest = super::crypto_digest::Digest::fetch(&algorithm)
        .map_err(|error| errors::throw(context, operation, error))?;
    let output = digest
        .hmac(&key, &input)
        .map_err(|error| errors::throw(context, operation, error))?;
    let roots = output
        .into_iter()
        .map(|byte| context.number(f64::from(byte)))
        .collect::<Vec<_>>();
    context.array_rooted(&roots)
}
