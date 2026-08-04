{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "events") {
        result.EventEmitterAsyncResource ||= result.EventEmitter;
        result.addAbortListener ||= () => () => undefined;
        result.getEventListeners ||= () => [];
        result.getMaxListeners ||= () => 10;
        result.setMaxListeners ||= () => undefined;
        result.listenerCount ||= () => 0;
      }
      return result;
    };
  }
}
