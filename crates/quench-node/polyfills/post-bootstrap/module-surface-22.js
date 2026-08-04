{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = String(name).replace(/^node:/, "");
      if (normalized === "readline" || normalized === "readline/promises") {
        const Interface = function Interface() {};
        const createInterface = (options) => {
          const listeners = options?.input;
          options?.output?.write?.("");
          return {
            question: async (prompt) => {
              options?.output?.write?.(prompt);
              return await new Promise((resolve) =>
                listeners?.once?.("line", resolve)
              );
            },
            close: () => options?.input?.pause?.()
          };
        };
        return normalized === "readline/promises"
          ? { Interface, createInterface }
          : {
              createInterface,
              emitKeypressEvents: () => undefined,
              cursorTo: () => undefined,
              moveCursor: () => undefined,
              clearLine: () => undefined,
              Interface,
              ReadStream: function ReadStream() {},
              WriteStream: function WriteStream() {}
            };
      }
      return originalRequire(name);
    };
  }
}
