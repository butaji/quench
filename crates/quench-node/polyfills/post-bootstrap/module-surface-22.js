const __quenchReadlineInterface = (options = {}) => {
  const listeners = new Map();
  let buffer = "";
  const interfaceObject = {
    cursor: 0,
    on(event, listener) {
      (listeners.get(event) || listeners.set(event, []).get(event)).push(
        listener,
      );
      return interfaceObject;
    },
    emit(event, ...args) {
      for (const listener of listeners.get(event) || []) listener(...args);
      return interfaceObject;
    },
    close() {
      options.input?.pause?.();
      return interfaceObject;
    },
    write(data) {
      interfaceObject.cursor += String(data).length;
      return interfaceObject;
    },
  };
  const emitLines = (final) => {
    const parts = buffer.split(/\r?\n/);
    buffer = final ? "" : parts.pop();
    for (const line of parts) interfaceObject.emit("line", line);
    if (final && parts.length === 0 && buffer) {
      interfaceObject.emit("line", buffer);
    }
  };
  options.input?.on?.("data", (chunk) => {
    buffer += String(chunk);
    emitLines(false);
  });
  options.input?.on?.("end", () => emitLines(true));
  return interfaceObject;
};
{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const normalized = String(name).replace(/^node:/, "");
      if (normalized === "readline" || normalized === "readline/promises") {
        const Interface = function Interface() {};
        const createInterface = (options) => {
          if (normalized === "readline") {
            return __quenchReadlineInterface(options);
          }
          const listeners = options?.input;
          options?.output?.write?.("");
          return {
            question: async (prompt) => {
              options?.output?.write?.(prompt);
              return await new Promise((resolve) =>
                listeners?.once?.("line", resolve)
              );
            },
            close: () => options?.input?.pause?.(),
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
            WriteStream: function WriteStream() {},
          };
      }
      return originalRequire(name);
    };
  }
}
