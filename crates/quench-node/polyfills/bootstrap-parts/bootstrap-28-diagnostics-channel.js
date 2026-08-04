const __quenchOriginalRequireWithDiagnostics = globalThis.require;
const __quenchChannels = new Map();
const __quenchChannel = (name) => {
  if (__quenchChannels.has(name)) return __quenchChannels.get(name);
  const subscribers = new Set(),
    stores = new Set();
  const channel = {
    name,
    get hasSubscribers() {
      return subscribers.size > 0;
    },
    subscribe: (callback) => subscribers.add(callback),
    unsubscribe: (callback) => subscribers.delete(callback),
    publish: (message, context) => {
      for (const callback of subscribers) callback(message, context);
    },
    bindStore: (store) => stores.add(store),
    unbindStore: (store) => stores.delete(store),
    runStores: (message, callback) => {
      const values = [...stores].map((store) => store.enterWith?.(message));
      try {
        return callback();
      } finally {
        values.length = 0;
      }
    }
  };
  __quenchChannels.set(name, channel);
  return channel;
};
const __quenchDiagnostics = {
  channel: __quenchChannel,
  hasSubscribers: (name) => __quenchChannel(name).hasSubscribers,
  subscribe: (name, callback) => __quenchChannel(name).subscribe(callback),
  unsubscribe: (name, callback) => __quenchChannel(name).unsubscribe(callback),
  tracingChannel: (name) => ({
    start: __quenchChannel(`${name}:start`),
    end: __quenchChannel(`${name}:end`),
    asyncStart: __quenchChannel(`${name}:asyncStart`),
    asyncEnd: __quenchChannel(`${name}:asyncEnd`),
    error: __quenchChannel(`${name}:error`)
  })
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "diagnostics_channel")
    return __quenchDiagnostics;
  return __quenchOriginalRequireWithDiagnostics(specifier);
};
