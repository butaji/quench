{
  if (globalThis.require) {
    for (const name of ["events", "node:events"]) {
      const eventsApi = globalThis.require(name);
      eventsApi.EventEmitterAsyncResource ||= eventsApi.EventEmitter;
      eventsApi.addAbortListener ||= () => () => undefined;
      eventsApi.getEventListeners ||= () => [];
      eventsApi.getMaxListeners ||= () => 10;
      eventsApi.setMaxListeners ||= () => undefined;
      const listenerCount = eventsApi.listenerCount;
      eventsApi.listenerCount = (target, event, listener) => {
        if (target instanceof AbortSignal)
          return target.listenerCount?.(event) || 0;
        return listenerCount(target, event, listener);
      };
    }
  }
}
