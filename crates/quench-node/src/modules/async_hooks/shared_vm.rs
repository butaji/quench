//! Shared-VM Node async-hooks surface for the implemented host primitives.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const API: &str = quench_js_check::checked_js!(
    r#"(() => {
  let nextAsyncId = 1;
  const localStates = new WeakMap();
  const activeStores = new Map();
  const resourceStates = new WeakMap();

  class AsyncLocalStorage {
    constructor() {
      localStates.set(this, { store: undefined, active: false });
    }

    disable() {
      const state = localStates.get(this);
      if (!state) throw new TypeError('AsyncLocalStorage method called on an incompatible receiver');
      activeStores.delete(this);
      return this;
    }

    getStore() {
      const state = localStates.get(this);
      return state && activeStores.has(this) ? activeStores.get(this) : undefined;
    }

    run(store, callback, ...args) {
      if (typeof callback !== 'function') throw new TypeError('callback must be a function');
      const state = localStates.get(this);
      if (!state) throw new TypeError('AsyncLocalStorage method called on an incompatible receiver');
      const hadPrevious = activeStores.has(this);
      const previous = activeStores.get(this);
      activeStores.set(this, store);
      try {
        return Reflect.apply(callback, undefined, args);
      } finally {
        if (hadPrevious) activeStores.set(this, previous);
        else activeStores.delete(this);
      }
    }

    enterWith(store) {
      const state = localStates.get(this);
      if (!state) throw new TypeError('AsyncLocalStorage method called on an incompatible receiver');
      activeStores.set(this, store);
    }

    exit(callback, ...args) {
      const state = localStates.get(this);
      if (!state) throw new TypeError('AsyncLocalStorage method called on an incompatible receiver');
      const hadPrevious = activeStores.has(this);
      const previous = activeStores.get(this);
      activeStores.delete(this);
      try {
        return typeof callback === 'function' ? Reflect.apply(callback, undefined, args) : undefined;
      } finally {
        if (hadPrevious) activeStores.set(this, previous);
        else activeStores.delete(this);
      }
    }

    static bind(callback) {
      const stores = Array.from(activeStores.entries());
      return function(...args) {
        const receiver = this;
        let invoke = () => Reflect.apply(callback, receiver, args);
        for (const [instance, store] of stores) {
          const next = invoke;
          invoke = () => instance.run(store, next);
        }
        return invoke();
      };
    }
  }

  class AsyncResource {
    constructor(type, options = undefined) {
      resourceStates.set(this, {
        type: String(type),
        asyncId: nextAsyncId++,
        triggerAsyncId: options?.triggerAsyncId ?? 0,
      });
    }

    runInAsyncScope(callback, thisArg, ...args) {
      if (typeof callback !== 'function') throw new TypeError('callback must be a function');
      if (!resourceStates.has(this)) throw new TypeError('AsyncResource method called on an incompatible receiver');
      return Reflect.apply(callback, thisArg, args);
    }

    emitDestroy() { return this; }
    asyncId() { return resourceStates.get(this)?.asyncId ?? 0; }
    triggerAsyncId() { return resourceStates.get(this)?.triggerAsyncId ?? 0; }
    type() { return resourceStates.get(this)?.type ?? 'UNKNOWN'; }

    bind(callback, thisArg = undefined) {
      return (...args) => this.runInAsyncScope(callback, thisArg, ...args);
    }

    static bind(callback, type, thisArg = undefined) {
      return new AsyncResource(type ?? 'bound-anonymous-fn').bind(callback, thisArg);
    }
  }

  const createHook = () => ({ enable() { return this; }, disable() { return this; } });
  return {
    AsyncLocalStorage,
    AsyncResource,
    createHook,
    executionAsyncId: () => 1,
    triggerAsyncId: () => 0,
    executionAsyncResource: () => undefined,
  };
})()"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    context.evaluate_script_rooted(API, "node:async_hooks/shared-api.js")
}
