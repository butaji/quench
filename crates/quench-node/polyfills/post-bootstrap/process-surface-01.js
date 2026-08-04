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
    globalThis.process.stdout ||= {};
    globalThis.process.stdout.fd ??= 1;
    globalThis.process.stdout._isStdio ??= true;
    globalThis.process.stdout.destroyed ??= false;
    globalThis.process.stdout.writable ??= true;
    globalThis.process.stdout.writableEnded ??= false;
    globalThis.process.stdout.writableFinished ??= false;
    globalThis.process.stdout.writableNeedDrain ??= false;
    globalThis.process.stdout.writableHighWaterMark ??= 16384;
    globalThis.process.stdout.readable ??= false;
    globalThis.process.stdout.readableEnded ??= true;
    globalThis.process.stdout.readableFlowing ??= null;
    globalThis.process.stdout.readableHighWaterMark ??= 65536;
    globalThis.process.stdout.readableLength ??= 0;
    globalThis.process.stdout.bytesWritten ??= 0;
    globalThis.process.stdout.writableCorked ??= 0;
    globalThis.process.stdout.pending ??= false;
    globalThis.process.stdout.writableObjectMode ??= false;
    globalThis.process.stdout.readableObjectMode ??= false;
    globalThis.process.stdout.write ||= write;
    globalThis.process.stdout.on ||= on;
    globalThis.process.stdout.addListener ||= on;
    globalThis.process.stdout.prependListener ||= on;
    globalThis.process.stdout.once ||= once;
    globalThis.process.stdout.prependOnceListener ||= once;
    globalThis.process.stdout.removeListener ||= removeListener;
    globalThis.process.stdout.off ||= removeListener;
    globalThis.process.stdout.emit ||= emit;
    globalThis.process.stdout.listenerCount ||= listenerCount;
    globalThis.process.stdout.eventNames ||= eventNames;
    globalThis.process.stdout.rawListeners ||= rawListeners;
    globalThis.process.stdout.listeners ||= listeners;
    globalThis.process.stdout.getMaxListeners ||= getMaxListeners;
    globalThis.process.stdout.setMaxListeners ||= setMaxListeners;
    globalThis.process.stdout[Symbol.asyncIterator] ||= asyncIterator;
    globalThis.process.stdout.destroy ||= destroy;
    globalThis.process.stdout.destroySoon ||= destroySoon;
    globalThis.process.stdout.ref ||= ref;
    globalThis.process.stdout.unref ||= unref;
    globalThis.process.stdout.setDefaultEncoding ||= setDefaultEncoding;
    globalThis.process.stdout.setEncoding ||= setEncoding;
    globalThis.process.stdout.end ||= end;
    globalThis.process.stdout.cork ||= cork;
    globalThis.process.stdout.uncork ||= uncork;
    globalThis.process.stderr ||= {};
    globalThis.process.stderr.fd ??= 2;
    globalThis.process.stderr._isStdio ??= true;
    globalThis.process.stderr.destroyed ??= false;
    globalThis.process.stderr.writable ??= true;
    globalThis.process.stderr.writableEnded ??= false;
    globalThis.process.stderr.writableFinished ??= false;
    globalThis.process.stderr.writableNeedDrain ??= false;
    globalThis.process.stderr.writableHighWaterMark ??= 16384;
    globalThis.process.stderr.readable ??= false;
    globalThis.process.stderr.readableEnded ??= true;
    globalThis.process.stderr.readableFlowing ??= null;
    globalThis.process.stderr.readableHighWaterMark ??= 65536;
    globalThis.process.stderr.readableLength ??= 0;
    globalThis.process.stderr.bytesWritten ??= 0;
    globalThis.process.stderr.writableCorked ??= 0;
    globalThis.process.stderr.pending ??= false;
    globalThis.process.stderr.writableObjectMode ??= false;
    globalThis.process.stderr.readableObjectMode ??= false;
    globalThis.process.stderr.write ||= write;
    globalThis.process.stderr.on ||= on;
    globalThis.process.stderr.addListener ||= on;
    globalThis.process.stderr.prependListener ||= on;
    globalThis.process.stderr.once ||= once;
    globalThis.process.stderr.prependOnceListener ||= once;
    globalThis.process.stderr.removeListener ||= removeListener;
    globalThis.process.stderr.off ||= removeListener;
    globalThis.process.stderr.emit ||= emit;
    globalThis.process.stderr.listenerCount ||= listenerCount;
    globalThis.process.stderr.eventNames ||= eventNames;
    globalThis.process.stderr.rawListeners ||= rawListeners;
    globalThis.process.stderr.listeners ||= listeners;
    globalThis.process.stderr.getMaxListeners ||= getMaxListeners;
    globalThis.process.stderr.setMaxListeners ||= setMaxListeners;
    globalThis.process.stderr[Symbol.asyncIterator] ||= asyncIterator;
    globalThis.process.stderr.destroy ||= destroy;
    globalThis.process.stderr.destroySoon ||= destroySoon;
    globalThis.process.stderr.ref ||= ref;
    globalThis.process.stderr.unref ||= unref;
    globalThis.process.stderr.setDefaultEncoding ||= setDefaultEncoding;
    globalThis.process.stderr.setEncoding ||= setEncoding;
    globalThis.process.stderr.end ||= end;
    globalThis.process.stderr.cork ||= cork;
    globalThis.process.stderr.uncork ||= uncork;
    globalThis.process.stdin ||= new globalThis.__nodeEventEmitter();
    globalThis.process.stdin.readable ??= true;
    globalThis.process.stdin.readableEnded ??= false;
    globalThis.process.stdin.readableFlowing ??= null;
    globalThis.process.stdin.pause ||= () => globalThis.process.stdin;
    globalThis.process.stdin.resume ||= () => globalThis.process.stdin;
    globalThis.process.stdin.setEncoding ||= () => globalThis.process.stdin;
  }
}
