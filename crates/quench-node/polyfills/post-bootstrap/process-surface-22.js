{
  if (globalThis.process) {
    globalThis.process.openStdin ||= () => globalThis.process.stdin;
    globalThis.process.constrainedMemory ||= () => Number.MAX_SAFE_INTEGER;
    globalThis.process.threadCpuUsage ||= (previous) => {
      if (
        previous !== undefined &&
        (typeof previous !== "object" ||
          previous === null ||
          Array.isArray(previous))
      ) {
        throw Object.assign(
          new TypeError("The prevValue argument must be an object"),
          { code: "ERR_INVALID_ARG_TYPE" }
        );
      }
      return { user: 0, system: 0 };
    };
  }
}
