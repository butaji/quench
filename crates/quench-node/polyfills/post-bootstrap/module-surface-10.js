{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "crypto") {
        result = Object.assign({}, result);
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
