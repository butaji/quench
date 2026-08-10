{
  if (globalThis.process) {
    const write = (chunk) => {
      globalThis.__quench_console_write(String(chunk));
      return true;
    };
    const on = function () {
      return this;
    };
    const once = function () {
      return this;
    };
    const removeListener = function () {
      return this;
    };
    const emit = function () {
      return false;
    };
    const listenerCount = function () {
      return 0;
    };
    const eventNames = function () {
      return [];
    };
    const rawListeners = function () {
      return [];
    };
    const listeners = function () {
      return [];
    };
    const asyncIterator = function () {
      return { next: async () => ({ done: true, value: undefined }) };
    };
    const destroy = function () {
      return this;
    };
    const ref = function () {
      return this;
    };
    const unref = function () {
      return this;
    };
    const setDefaultEncoding = function () {
      return this;
    };
    const destroySoon = function () {};
    const listenerLimits = new WeakMap();
    const getMaxListeners = function () {
      return listenerLimits.get(this) ?? 10;
    };
    const setMaxListeners = function (limit) {
      listenerLimits.set(this, Number(limit));
      return this;
    };
    const setEncoding = function () {
      return this;
    };
    const end = function () {
      return this;
    };
    const cork = function () {
      return this;
    };
    const uncork = function () {
      return this;
    };
    const configure = (stream) => {
      stream.write ||= write;
      stream.on ||= on;
      stream.addListener ||= on;
      stream.prependListener ||= on;
      stream.once ||= once;
      stream.prependOnceListener ||= once;
      stream.removeListener ||= removeListener;
      stream.off ||= removeListener;
      stream.emit ||= emit;
      stream.listenerCount ||= listenerCount;
      stream.eventNames ||= eventNames;
      stream.rawListeners ||= rawListeners;
      stream.listeners ||= listeners;
      stream.getMaxListeners ||= getMaxListeners;
      stream.setMaxListeners ||= setMaxListeners;
      stream[Symbol.asyncIterator] ||= asyncIterator;
      stream.destroy ||= destroy;
      stream.destroySoon ||= destroySoon;
      stream.ref ||= ref;
      stream.unref ||= unref;
      stream.setDefaultEncoding ||= setDefaultEncoding;
      stream.setEncoding ||= setEncoding;
      stream.end ||= end;
      stream.cork ||= cork;
      stream.uncork ||= uncork;
    };
    const configureStream = (stream, fd) => {
      stream.fd ??= fd;
      stream._isStdio ??= true;
      stream.destroyed ??= false;
      stream.writable ??= true;
      stream.writableEnded ??= false;
      stream.writableFinished ??= false;
      stream.writableNeedDrain ??= false;
      stream.writableHighWaterMark ??= 16384;
      stream.readable ??= false;
      stream.readableEnded ??= true;
      stream.readableFlowing ??= null;
      stream.readableHighWaterMark ??= 65536;
      stream.readableLength ??= 0;
      stream.bytesWritten ??= 0;
      stream.writableCorked ??= 0;
      stream.pending ??= false;
      stream.writableObjectMode ??= false;
      stream.readableObjectMode ??= false;
      configure(stream);
    };
    for (const [name, fd] of [
      ["stdout", 1],
      ["stderr", 2]
    ]) {
      globalThis.process[name] ||= {};
      configureStream(globalThis.process[name], fd);
    }
    globalThis.process.stdin ||= new globalThis.__nodeEventEmitter();
    globalThis.process.stdin.readable ??= true;
    globalThis.process.stdin.readableEnded ??= false;
    globalThis.process.stdin.readableFlowing ??= null;
    globalThis.process.stdin.pause ||= () => globalThis.process.stdin;
    globalThis.process.stdin.resume ||= () => globalThis.process.stdin;
    globalThis.process.stdin.setEncoding ||= () => globalThis.process.stdin;
    globalThis.process.stdin.readableHighWaterMark ??= 65536;
    globalThis.process.stdin.readableLength ??= 0;
    globalThis.process.stdin.readableObjectMode ??= false;
    globalThis.process.stdin.read ||= () => null;
    globalThis.process.stdin.unshift ||= () => globalThis.process.stdin;
    globalThis.process.stdin.isPaused ||= () => false;
    const stdin = globalThis.process.stdin;
    stdin.destroy ||= () => stdin;
    stdin.ref ||= () => stdin;
    stdin.unref ||= () => stdin;
    stdin.fd ??= 0;
    stdin.destroyed ??= false;
    stdin.readableEncoding ??= null;
    stdin.closed ??= false;
    stdin.errored ??= null;
    stdin.readableAborted ??= false;
    stdin.autoClose ??= false;
    stdin.bytesRead ??= 0;
    stdin.pipe ||= (destination) => destination;
    stdin.unpipe ||= () => stdin;
    stdin.wrap ||= () => stdin;
    stdin.close ||= () => stdin;
    stdin.pending ??= false;
    stdin[Symbol.asyncDispose] ||= async () => undefined;
    if (stdin.constructor.name !== "ReadStream") {
      Object.defineProperty(stdin, "constructor", {
        value: function ReadStream() {},
        configurable: true
      });
    }
    stdin.end ??= null;
    const dispose = async () => undefined;
    globalThis.process.stdout[Symbol.asyncDispose] ||= dispose;
    globalThis.process.stderr[Symbol.asyncDispose] ||= dispose;
    globalThis.process.getgroups ||= () => [0];
    globalThis.process.initgroups ||= () => undefined;
    globalThis.process.setgroups ||= () => undefined;
    globalThis.process.setegid ||= () => undefined;
    globalThis.process.seteuid ||= () => undefined;
    globalThis.process.getegid ||= () => 0;
    globalThis.process.geteuid ||= () => 0;
    globalThis.process.emitWarning ||= () => undefined;
    globalThis.process._getActiveHandles ||= () => [];
    globalThis.process._getActiveRequests ||= () => [];
    globalThis.process.kill ||= () => true;
    globalThis.process.abort ||= () => undefined;
    globalThis.process.execve ||= () => undefined;
    globalThis.process.reallyExit ||= () => undefined;
    globalThis.process.ref ||= () => undefined;
    globalThis.process.unref ||= () => undefined;
    if (
      globalThis.process.allowedNodeEnvironmentFlags instanceof Set &&
      globalThis.process.allowedNodeEnvironmentFlags.size === 0
    ) {
      globalThis.process.allowedNodeEnvironmentFlags.add("--no-warnings");
    }
    if (globalThis.process.hrtime) {
      globalThis.process.hrtime.bigint ||= () => BigInt(Date.now()) * 1000000n;
    }
    const config = (globalThis.process.config ||= {});
    config.variables ||= {};
    config.target_defaults ||= {};
    const report = (globalThis.process.report ||= {});
    report.compact ??= false;
    report.directory ??= "";
    report.excludeEnv ??= false;
    report.excludeNetwork ??= false;
    report.filename ??= "";
    report.reportOnFatalError ??= false;
    report.reportOnSignal ??= false;
    report.reportOnUncaughtException ??= false;
    report.signal ??= "SIGUSR2";
    report.getReport ||= () => ({});
    report.writeReport ||= () => undefined;
  }
}
