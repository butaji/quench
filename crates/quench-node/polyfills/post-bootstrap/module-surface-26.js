{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      let result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "constants") {
        result = Object.assign({}, result);
        result.errno ||= {};
        result.signals ||= {};
        result.os ||= {};
        result.fs ||= {};
        result.crypto ||= {};
        result.zlib ||= {};
        result.O_RDONLY ??= 0;
        result.SIGTERM ??= 15;
        result = Object.freeze(result);
      }
      return result;
    };
  }
}
