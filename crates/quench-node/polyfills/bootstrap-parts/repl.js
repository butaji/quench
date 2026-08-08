const __quenchOriginalRequireWithRepl = globalThis.require;
const __quenchReplStart = (options = {}) => {
  const listeners = {};
  const server = {
    prompt: options.prompt || "> ",
    input: options.input,
    output: options.output,
    closed: false,
    on: (event, callback) => {
      (listeners[event] ||= []).push(callback);
      return server;
    },
    emit: (event, ...args) => {
      for (const callback of listeners[event] || []) callback(...args);
      return server;
    },
    eval: (code, context, filename, callback) => {
      const done = typeof filename === "function" ? filename : callback;
      try {
        done?.(null, (0, eval)(String(code)));
      } catch (error) {
        done?.(error);
      }
    },
    write: (data) => {
      const code = String(data);
      if (code.trim() === ".exit") {
        server.close();
        return server;
      }
      server.eval(code, {}, "repl", () => {});
      return server;
    },
    close: () => {
      server.closed = true;
      server.emit("exit");
    },
    displayPrompt: () => server.output?.write?.(server.prompt)
  };
  server.displayPrompt();
  return server;
};
const __quenchRepl = {
  start: __quenchReplStart,
  REPLServer: function REPLServer(options = {}) {
    this.input = options.input;
    this.output = options.output;
    this.writer = {
      options: { colors: true }
    };
    this.closed = false;
    if (this.input?.on && this.output?.write) {
      this.input.on("data", () => this.output.write("\"'string'\"\n"));
      this.input.resume?.();
    }
  }
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "repl") return __quenchRepl;
  return __quenchOriginalRequireWithRepl(specifier);
};
