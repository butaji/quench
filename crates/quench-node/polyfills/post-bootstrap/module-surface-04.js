{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "util") {
        result.parseEnv ||= () => ({});
        result.inherits ||= (constructor, superConstructor) => {
          Object.setPrototypeOf(
            constructor.prototype,
            superConstructor.prototype
          );
        };
        result.MIMEType ||= function MIMEType() {};
        result.isDeepStrictEqual ||= (left, right) => left === right;
      }
      return result;
    };
  }
}
