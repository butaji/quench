{
  if (globalThis.require) {
    const originalRequire = globalThis.require;
    globalThis.require = (name) => {
      const result = originalRequire(name);
      if (String(name).replace(/^node:/, "") === "async_hooks") {
        result.createHook ||= () => ({
          enable: () => undefined,
          disable: () => undefined
        });
        result.executionAsyncId ||= () => 0;
        result.triggerAsyncId ||= () => 0;
        result.executionAsyncResource ||= () => ({});
        result.AsyncResource ||= function AsyncResource() {};
        result.AsyncLocalStorage ||= function AsyncLocalStorage() {};
      }
      return result;
    };
  }
}
