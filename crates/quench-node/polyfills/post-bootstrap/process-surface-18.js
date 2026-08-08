{
  if (globalThis.process) {
    let currentUmask = 0o22;
    globalThis.process.umask = (value) => {
      if (value === undefined) return currentUmask;
      if (typeof value === "string") {
        if (!/^[0-7]+$/.test(value)) {
          throw Object.assign(new RangeError("Invalid umask value"), {
            code: "ERR_INVALID_ARG_VALUE"
          });
        }
        value = parseInt(value, 8);
      }
      if (typeof value !== "number" || !Number.isInteger(value)) {
        throw Object.assign(
          new TypeError("The mode argument must be of type number or string"),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      const previous = currentUmask;
      currentUmask = value & 0o777;
      return previous;
    };
    globalThis.process.getgid ||= () => 0;
    globalThis.process.getuid ||= () => 0;
    const setCredential = (id) => {
      if (typeof id !== "number" && typeof id !== "string") {
        throw Object.assign(
          new TypeError("The id argument must be one of type number or string"),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
    };
    globalThis.process.setgid = setCredential;
    globalThis.process.setuid = setCredential;
  }
}
