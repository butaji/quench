{
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
        result.pbkdf2 ||= (
          password,
          salt,
          iterations,
          length,
          digest,
          callback
        ) => {
          Promise.resolve().then(() =>
            callback(
              null,
              result.pbkdf2Sync(digest, password, salt, iterations, length)
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
