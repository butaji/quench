//! Polyfill: `module-surface-10`

pub const JS: &str = quench_js_check::checked_js!(r#"{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "crypto") {
        const source = result;
        result = Object.assign({}, source);
        for (const name of ["pseudoRandomBytes", "prng", "rng"]) {
          const descriptor = Object.getOwnPropertyDescriptor(source, name);
          if (descriptor) Object.defineProperty(result, name, descriptor);
        }
        result.pbkdf2Sync ||= (password, salt, iterations, keylen, digest) => {
          if (typeof digest !== "string") {
            throw Object.assign(new TypeError('The "digest" argument must be of type string'), { code: "ERR_INVALID_ARG_TYPE" });
          }
          if (!Number.isInteger(iterations) || iterations <= 0 || iterations > 0x7fffffff) {
            throw Object.assign(new RangeError(`The value of "iterations" is out of range. It must be >= 1 && <= 2147483647. Received ${iterations}`), { code: "ERR_OUT_OF_RANGE" });
          }
          if (!Number.isInteger(keylen) || keylen < 0 || keylen > 0x7fffffff) {
            const received = keylen === Infinity ? "Infinity" : keylen;
            throw Object.assign(new RangeError(`The value of "keylen" is out of range. It must be an integer. Received ${received}`), { code: "ERR_OUT_OF_RANGE" });
          }
          if (digest.toLowerCase() !== "sha256" && digest.toLowerCase() !== "sha1") {
            throw Object.assign(new TypeError(`Invalid digest: ${digest}`), { code: "ERR_CRYPTO_INVALID_DIGEST" });
          }
          const bytes = (value) => typeof value === "string" ? Array.from(new NodeTextEncoder().encode(value)) : Array.from(value);
          return NodeBuffer.from(globalThis.__quench_pbkdf2_bytes(bytes(password), bytes(salt), iterations, keylen));
        };
        result.pbkdf2 ||= (
          password,
          salt,
          iterations,
          length,
          digest,
          callback,
        ) => {
          Promise.resolve().then(() =>
            callback(
              null,
              result.pbkdf2Sync(password, salt, iterations, length, digest),
            )
          );
        };
        const originalHkdf = result.hkdf;
        result.hkdf = (digest, ikm, salt, info, length, callback) => {
          const crypto = globalThis.require("crypto");
          const derived = crypto.hkdfSync(digest, ikm, salt, info, length);
          if (typeof originalHkdf === "function") {
            return originalHkdf(digest, ikm, salt, info, length, callback);
          }
          Promise.resolve().then(() => callback(null, derived));
        };
      }
      return result;
    };
  }
}
"#);
