{
  if (globalThis.process) {
    globalThis.process.openStdin ||= () => globalThis.process.stdin;
    globalThis.process.constrainedMemory ||= () => Number.MAX_SAFE_INTEGER;
    globalThis.process.threadCpuUsage ||= (previous) => {
      if (typeof previous === "number") {
        throw Object.assign(
          new TypeError("The prevValue argument must be an object"),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      if (Array.isArray(previous)) {
        throw Object.assign(
          new TypeError("The prevValue argument must be an object"),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      if (
        previous !== undefined &&
        (typeof previous !== "object" || previous === null)
      ) {
        throw Object.assign(
          new TypeError("The prevValue argument must be an object"),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      return { user: 0, system: 0 };
    };
  }
}
