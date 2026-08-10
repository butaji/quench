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
  let currentGid =
    typeof globalThis.process.getgid === "function"
      ? globalThis.process.getgid()
      : 0;
  let currentUid =
    typeof globalThis.process.getuid === "function"
      ? globalThis.process.getuid()
      : 0;
  globalThis.process.getgid = () => currentGid;
  globalThis.process.getuid = () => currentUid;
  const credentialId = (id) => {
    if (typeof id !== "number" && typeof id !== "string") {
      throw Object.assign(
        new TypeError("The id argument must be one of type number or string"),
        { code: "ERR_INVALID_ARG_TYPE" }
      );
    }
    if (typeof id === "string") {
      if (id === "root") return 0;
      if (id === "nobody") return 65534;
      if (!/^[0-9]+$/.test(id)) {
        throw Object.assign(new Error(`Group or user ${id} does not exist`), {
          code: "ESRCH"
        });
      }
      id = Number(id);
    }
    if (!Number.isSafeInteger(id) || id < 0) {
      throw Object.assign(new RangeError("The id argument is out of range"), {
        code: "ERR_OUT_OF_RANGE"
      });
    }
    return id;
  };
  globalThis.process.setgid = (id) => {
    currentGid = credentialId(id);
  };
  globalThis.process.setuid = (id) => {
    currentUid = credentialId(id);
  };
}
