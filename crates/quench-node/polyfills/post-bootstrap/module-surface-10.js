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
        result.hkdf ||= (digest, ikm, salt, info, length, callback) => {
          Promise.resolve().then(() => {
            const crypto = globalThis.require("crypto");
            callback(null, crypto.hkdfSync(digest, ikm, salt, info, length));
          });
        };
      }
      return result;
    };
  }
}
