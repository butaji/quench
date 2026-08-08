const __quenchOriginalRequireWithDiagnostics = globalThis.require;
const __quenchChannels = new Map();
class __QuenchChannelClass {}
const __quenchChannel = (name) => {
  if (__quenchChannels.has(name)) return __quenchChannels.get(name);
  const subscribers = new Set(),
    stores = new Map();
  const channel = {
    name,
    get hasSubscribers() {
      return subscribers.size > 0 || stores.size > 0;
    },
    subscribe: (callback) => {
      if (typeof callback !== "function") {
        const error = new TypeError("The subscription must be a function");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      subscribers.add(callback);
    },
    unsubscribe: (callback) => {
      if (typeof callback !== "function") {
        const error = new TypeError("The subscription must be a function");
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      return subscribers.delete(callback);
    },
    publish: (message, context) => {
      for (const callback of subscribers) {
        callback(message, context === undefined ? name : context);
      }
    },
    bindStore: (store, transform) => stores.set(store, transform),
    unbindStore: (store) => stores.delete(store),
    runStores: (message, callback, thisArg, ...args) => {
      const previous = [...stores].map(([store, transform]) => [
        store,
        store.getStore?.(),
        typeof transform === "function" ? transform(message) : message
      ]);
      for (const [store, , value] of previous) store.enterWith?.(value);
      try {
        return callback.apply(thisArg, args);
      } finally {
        for (const [store, previousValue] of previous) {
          store.enterWith?.(previousValue);
        }
      }
    },
    withStoreScope: (message = {}) => {
      const previous = [...stores].map(([store, transform]) => [
        store,
        store.getStore?.(),
        typeof transform === "function" ? transform(message) : message
      ]);
      for (const [store, , value] of previous) store.enterWith?.(value);
      channel.publish(message);
      let active = true;
      return {
        [Symbol.dispose || Symbol("dispose")]() {
          if (!active) return;
          active = false;
          for (const [store, previousValue] of previous) {
            store.enterWith?.(previousValue);
          }
        }
      };
    }
  };
  Object.setPrototypeOf(channel, __QuenchChannelClass.prototype);
  __quenchChannels.set(name, channel);
  return channel;
};
const __quenchDiagnostics = {
  Channel: __QuenchChannelClass,
  channel: __quenchChannel,
  hasSubscribers: (name) => __quenchChannel(name).hasSubscribers,
  subscribe: (name, callback) => __quenchChannel(name).subscribe(callback),
  unsubscribe: (name, callback) => __quenchChannel(name).unsubscribe(callback),
  BoundedChannel: class BoundedChannel {
    constructor(nameOrChannels) {
      const channels =
        typeof nameOrChannels === "string"
          ? {
              start: __quenchChannel(`tracing:${nameOrChannels}:start`),
              end: __quenchChannel(`tracing:${nameOrChannels}:end`)
            }
          : nameOrChannels;
      this.start = channels?.start;
      this.end = channels?.end;
    }
    get hasSubscribers() {
      return Boolean(this.start?.hasSubscribers || this.end?.hasSubscribers);
    }
    subscribe(handlers) {
      for (const name of ["start", "end"]) {
        if (handlers?.[name]) this[name]?.subscribe(handlers[name]);
      }
    }
    unsubscribe(handlers) {
      let done = true;
      for (const name of ["start", "end"]) {
        if (handlers?.[name] && !this[name]?.unsubscribe(handlers[name])) {
          done = false;
        }
      }
      return done;
    }
    withScope(context = {}) {
      if (!this.hasSubscribers) {
        return { [Symbol.dispose || Symbol("dispose")]() {} };
      }
      this.start?.runStores(context, () => this.start.publish(context));
      let active = true;
      const dispose = () => {
        if (!active) return;
        active = false;
        this.end?.publish(context);
      };
      Symbol.dispose ||= Symbol("Symbol.dispose");
      return { [Symbol.dispose]: dispose };
    }
    run(context, fn, thisArg, ...args) {
      const scope = this.withScope(context || {});
      try {
        return Reflect.apply(fn, thisArg, args);
      } finally {
        scope[Symbol.dispose]?.();
      }
    }
  },
  TracingChannel: class TracingChannel {
    constructor(nameOrChannels) {
      const channels =
        typeof nameOrChannels === "string"
          ? {
              start: __quenchChannel(`tracing:${nameOrChannels}:start`),
              end: __quenchChannel(`tracing:${nameOrChannels}:end`),
              asyncStart: __quenchChannel(
                `tracing:${nameOrChannels}:asyncStart`
              ),
              asyncEnd: __quenchChannel(`tracing:${nameOrChannels}:asyncEnd`),
              error: __quenchChannel(`tracing:${nameOrChannels}:error`)
            }
          : nameOrChannels;
      if (typeof nameOrChannels === "object") {
        for (const name of [
          "start",
          "end",
          "asyncStart",
          "asyncEnd",
          "error"
        ]) {
          if (
            channels?.[name] !== undefined &&
            !(channels[name] instanceof __QuenchChannelClass)
          ) {
            const error = new TypeError(
              `The "nameOrChannels.${name}" property must be an instance of Channel`
            );
            error.code = "ERR_INVALID_ARG_TYPE";
            throw error;
          }
        }
        if (!channels || Object.keys(channels).length === 0) {
          throw new TypeError("Cannot convert undefined or null to object");
        }
      }
      this.start = channels?.start;
      this.end = channels?.end;
      this.asyncStart = channels?.asyncStart;
      this.asyncEnd = channels?.asyncEnd;
      this.error = channels?.error;
    }
    get hasSubscribers() {
      return Boolean(
        this.start?.hasSubscribers ||
        this.end?.hasSubscribers ||
        this.asyncStart?.hasSubscribers ||
        this.asyncEnd?.hasSubscribers ||
        this.error?.hasSubscribers
      );
    }
    subscribe(handlers = {}) {
      for (const name of ["start", "end", "asyncStart", "asyncEnd", "error"]) {
        if (handlers[name]) this[name]?.subscribe(handlers[name]);
      }
    }
    unsubscribe(handlers = {}) {
      let done = true;
      for (const name of ["start", "end", "asyncStart", "asyncEnd", "error"]) {
        if (handlers[name] && !this[name]?.unsubscribe(handlers[name])) {
          done = false;
        }
      }
      return done;
    }
    traceSync(fn, context = {}, thisArg, ...args) {
      if (!this.hasSubscribers) return Reflect.apply(fn, thisArg, args);
      const scope = this.start?.withStoreScope(context);
      try {
        const result = Reflect.apply(fn, thisArg, args);
        context.result = result;
        return result;
      } catch (error) {
        context.error = error;
        this.error?.publish(context);
        throw error;
      } finally {
        this.end?.publish(context);
        scope?.[Symbol.dispose]?.();
      }
    }
    tracePromise(fn, context = {}, thisArg, ...args) {
      const result = this.traceSync(fn, context, thisArg, ...args);
      if (!result || typeof result.then !== "function") return result;
      const continuation = (value, error) => {
        if (error) {
          context.error = value;
          this.error?.publish(context);
        } else context.result = value;
        const scope = this.asyncStart?.withStoreScope(context);
        try {
          this.asyncEnd?.publish(context);
          if (error) throw value;
          return value;
        } finally {
          scope?.[Symbol.dispose]?.();
        }
      };
      return result.then(
        (value) => continuation(value, false),
        (error) => continuation(error, true)
      );
    }
    traceCallback(fn, position = -1, context = {}, thisArg, ...args) {
      if (!this.hasSubscribers) return Reflect.apply(fn, thisArg, args);
      const callbackIndex = position < 0 ? args.length + position : position;
      const callback = args[callbackIndex];
      if (typeof callback !== "function") {
        const error = new TypeError(
          'The "callback" argument must be of type function'
        );
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      args[callbackIndex] = (...callbackArgs) => {
        const asyncScope = this.asyncStart?.withStoreScope(context);
        if (callbackArgs[0]) context.error = callbackArgs[0];
        else context.result = callbackArgs[1];
        if (context.error) this.error?.publish(context);
        this.end?.publish(context);
        this.asyncEnd?.publish(context);
        try {
          return callback(...callbackArgs);
        } finally {
          asyncScope?.[Symbol.dispose]?.();
        }
      };
      const scope = this.start?.withStoreScope(context);
      try {
        return Reflect.apply(fn, thisArg, args);
      } catch (error) {
        context.error = error;
        this.error?.publish(context);
        throw error;
      } finally {
        scope?.[Symbol.dispose]?.();
      }
    }
  },
  boundedChannel: (nameOrChannels) =>
    new __quenchDiagnostics.BoundedChannel(nameOrChannels),
  tracingChannel: (name) => {
    if (typeof name !== "string" && (!name || typeof name !== "object")) {
      const error = new TypeError(
        'The "nameOrChannels" argument must be of type string or an instance of TracingChannel or Object'
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return new __quenchDiagnostics.TracingChannel(name);
  }
};
globalThis.__nodeDiagnosticsChannel = __quenchDiagnostics;
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "diagnostics_channel") {
    return __quenchDiagnostics;
  }
  return __quenchOriginalRequireWithDiagnostics(specifier);
};
