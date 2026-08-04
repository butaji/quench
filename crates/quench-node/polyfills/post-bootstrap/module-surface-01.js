{
  if (globalThis.require) {
    for (const name of ["events", "node:events"]) {
      const eventsApi = globalThis.require(name);
      eventsApi.EventEmitterAsyncResource ||= eventsApi.EventEmitter;
      eventsApi.addAbortListener ||= () => () => undefined;
      eventsApi.getEventListeners ||= () => [];
      eventsApi.getMaxListeners ||= () => 10;
      eventsApi.setMaxListeners ||= () => undefined;
      eventsApi.listenerCount ||= () => 0;
    }
  }
}
