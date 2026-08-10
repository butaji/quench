{
  const __quenchInheritsError = (message) => {
    const error = new TypeError(message);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };
  const __quenchValidateInherits = (constructor, superConstructor) => {
    if (typeof constructor !== "function") {
      throw __quenchInheritsError(
        `The "ctor" argument must be of type function. Received ${constructor}`,
      );
    }
    if (superConstructor == null) {
      throw __quenchInheritsError(
        `The "superCtor" argument must be of type function. Received ${superConstructor}`,
      );
    }
    if (
      typeof superConstructor !== "function" ||
      !superConstructor.prototype ||
      typeof superConstructor.prototype !== "object"
    ) {
      throw __quenchInheritsError(
        'The "superCtor.prototype" property must be of type object. Received undefined',
      );
    }
  };
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "util") {
        result.inherits ||= (constructor, superConstructor) => {
          __quenchValidateInherits(constructor, superConstructor);
          Object.setPrototypeOf(
            constructor.prototype,
            superConstructor.prototype,
          );
          Object.defineProperty(constructor.prototype, "constructor", {
            value: constructor,
            writable: true,
            configurable: true,
          });
          Object.defineProperty(constructor, "super_", {
            value: superConstructor,
            writable: true,
            configurable: true,
          });
          return constructor;
        };
        result.MIMEType ||= function MIMEType() {};
        result.isDeepStrictEqual ||= (left, right) => left === right;
      }
      return result;
    };
  }
}
