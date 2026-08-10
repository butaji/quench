//! Polyfill: `globals-tail`

pub const JS: &str = r#"process.hrtime.bigint = () => BigInt(globalThis.__quench_now_ns());
globalThis.setImmediate = (callback, ...args) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const resource = {
    asyncId: ++globalThis.__nodeNextAsyncId,
    triggerAsyncId: globalThis.__nodeCurrentAsyncResource?.asyncId || 1
  };
  for (const hook of globalThis.__nodeAsyncHooks || []) {
    if (typeof hook.callbacks?.init === "function") {
      hook.callbacks.init(
        resource.asyncId,
        "Immediate",
        resource.triggerAsyncId,
        resource
      );
    }
  }
  const id = {
    active: true,
    refed: true,
    generation: 0,
    __immediate: true,
    _destroyed: false
  };
  const activeDomain = globalThis.__quench_active_domain;
  id.ref = () => {
    if (!id.refed && id.active && !id.counted) {
      id.refed = true;
      id.counted = true;
      globalThis.__quenchRefedHandles++;
    }
    return id;
  };
  id.unref = () => ((id.refed = false), id);
  id.hasRef = () => id.active && id.refed;
  id.refresh = () => ((id.active = true), id);
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => {
    id.active = false;
    id._destroyed = true;
  };
  queueMicrotask(() => {
    if (id.active) {
      if (activeDomain) activeDomain.run(callback, ...args);
      else callback(...args);
    }
  });
  return id;
};
globalThis.clearImmediate = (id) => {
  if (id?.__immediate) {
    id.active = false;
    id._destroyed = true;
  }
};
globalThis.__quenchRefedHandles ||= 0;
globalThis.__quenchTimerHandleIds ||= new Map();
globalThis.__quenchNextTimerHandleId ||= 1;
globalThis.setTimeout = (callback, _delay = 0, ...args) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const id = {
    active: true,
    refed: true,
    generation: 0,
    counted: true,
    unrefChecks: 0,
    _destroyed: false
  };
  const handleId = globalThis.__quenchNextTimerHandleId++;
  globalThis.__quenchTimerHandleIds.set(handleId, id);
  id[Symbol.toPrimitive] = () => handleId;
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => globalThis.clearTimeout(id);
  globalThis.__quenchRefedHandles++;
  id.ref = () => {
    if (!id.refed && id.active && !id.counted) {
      id.refed = true;
      id.counted = true;
      globalThis.__quenchRefedHandles++;
    }
    return id;
  };
  id.unref = () => {
    if (id.refed && id.counted) {
      id.refed = false;
      id.counted = false;
      globalThis.__quenchRefedHandles = Math.max(
        0,
        globalThis.__quenchRefedHandles - 1
      );
    }
    return id;
  };
  id.hasRef = () => id.active && id.refed;
  const resource = globalThis.__nodeCurrentAsyncResource;
  const activeDomain = globalThis.__quench_active_domain;
  const schedule = () => {
    const generation = ++id.generation;
    queueMicrotask(() => {
      if (id.active && generation === id.generation) {
        if (
          !id.refed &&
          globalThis.__quenchRefedHandles > 0 &&
          id.unrefChecks++ < 1000
        ) {
          queueMicrotask(schedule);
          return;
        }
        if (!id.refed && globalThis.__quenchRefedHandles === 0) {
          id.active = false;
          return;
        }
        if (id.counted) {
          id.counted = false;
          globalThis.__quenchRefedHandles = Math.max(
            0,
            globalThis.__quenchRefedHandles - 1
          );
        }
        const delay = __nodeTimerDelay(_delay);
        if (delay) globalThis.__quench_sleep_ms(delay);
        const previous = globalThis.__nodeCurrentAsyncResource;
        globalThis.__nodeCurrentAsyncResource = resource;
        try {
          if (activeDomain) activeDomain.run(callback, ...args);
          else callback(...args);
        } finally {
          globalThis.__nodeCurrentAsyncResource = previous;
        }
      }
    });
  };
  id.refresh = () => {
    id.active = true;
    schedule();
    return id;
  };
  schedule();
  return id;
};
globalThis.clearTimeout = (id) => {
  if (typeof id === "number" || typeof id === "string") {
    const numericId = Number(id);
    if (Number.isInteger(numericId)) {
      id = globalThis.__quenchTimerHandleIds.get(numericId);
    }
  }
  if (id) {
    if (id.active && id.counted) {
      id.counted = false;
      globalThis.__quenchRefedHandles = Math.max(
        0,
        globalThis.__quenchRefedHandles - 1
      );
    }
    id.active = false;
    id._destroyed = true;
  }
};
globalThis.setInterval = (callback, _delay = 0, ...args) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  const id = { active: true, refed: true, generation: 0, _destroyed: false };
  const handleId = globalThis.__quenchNextTimerHandleId++;
  globalThis.__quenchTimerHandleIds.set(handleId, id);
  id[Symbol.toPrimitive] = () => handleId;
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => globalThis.clearInterval(id);
  id.ref = () => ((id.refed = true), id);
  id.unref = () => ((id.refed = false), id);
  id.hasRef = () => id.active && id.refed;
  const activeDomain = globalThis.__quench_active_domain;
  const schedule = () => {
    const generation = ++id.generation;
    queueMicrotask(() => {
      if (!id.active || generation !== id.generation) return;
      const delay = __nodeTimerDelay(_delay);
      if (delay) globalThis.__quench_sleep_ms(delay);
      if (activeDomain) activeDomain.run(() => callback.apply(id, args));
      else callback.apply(id, args);
      if (id.active) schedule();
    });
  };
  id.refresh = () => {
    id.active = true;
    schedule();
    return id;
  };
  Symbol.dispose ||= Symbol("dispose");
  id[Symbol.dispose] = () => globalThis.clearInterval(id);
  schedule();
  return id;
};
globalThis.clearInterval = globalThis.clearTimeout;
globalThis.__nodeTimers = {
  setTimeout,
  clearTimeout,
  setInterval,
  clearInterval,
  setImmediate,
  clearImmediate
};
if (typeof Object.hasOwn !== "function") {
  Object.defineProperty(Object, "hasOwn", {
    value: (object, property) =>
      Object.prototype.hasOwnProperty.call(object, property),
    configurable: true,
    writable: true
  });
}
if (typeof globalThis.Blob !== "function") {
  globalThis.Blob = class Blob {
    constructor(parts = [], options = {}) {
      if (!Array.isArray(parts)) {
        throw Object.assign(new TypeError('The "sources" argument must be an instance of Array'), { code: "ERR_INVALID_ARG_TYPE" });
      }
      const chunks = parts.map((part) =>
        typeof part === "string" ||
        ArrayBuffer.isView(part) ||
        part instanceof ArrayBuffer
          ? NodeBuffer.from(part)
          : part instanceof Blob
            ? NodeBuffer.from(part._data)
            : (() => {
                throw Object.assign(new TypeError("The sources argument contains an invalid part"), { code: "ERR_INVALID_ARG_TYPE" });
              })()
      );
      this._data = NodeBuffer.concat(chunks);
      this.size = this._data.length;
      this.type = String(options.type || "").toLowerCase();
    }
    async arrayBuffer() {
      return this._data.buffer.slice(
        this._data.byteOffset,
        this._data.byteOffset + this._data.byteLength
      );
    }
    async text() {
      return this._data.toString();
    }
    slice(start = 0, end = this.size, type = "") {
      return new Blob([this._data.subarray(start, end)], { type });
    }
  };
}
if (globalThis.Blob && !globalThis.Blob.__quenchTypeNormalized) {
  const __quenchNativeBlob = globalThis.Blob;
  const __quenchBlob = function Blob(parts = [], options = {}) {
    const value = new __quenchNativeBlob(parts, options);
    if (options && options.type !== undefined) {
      Object.defineProperty(value, "type", {
        configurable: true,
        value: String(options.type).toLowerCase()
      });
    }
    return value;
  };
  __quenchBlob.prototype = __quenchNativeBlob.prototype;
  Object.defineProperty(__quenchBlob, "__quenchTypeNormalized", {
    value: true
  });
  globalThis.Blob = __quenchBlob;
}
if (globalThis.process && typeof globalThis.process.emit !== "function") {
  globalThis.process.emit = () => globalThis.process;
}
if (globalThis.__quench_host_timer_scheduler) {
  const __quenchHostTimers = new Map();
  let __quenchHostTimerId = 1;
  let __quenchHostTimerOrder = 1;
  const __quenchHostNow = () =>
    Number(BigInt(globalThis.__quench_now_ns()) / 1000000n);
  const __quenchHostHandle = (entry) => {
    const handle = {
      active: true,
      refed: true,
      ref() {
        this.refed = true;
        return this;
      },
      unref() {
        this.refed = false;
        return this;
      },
      hasRef() {
        return this.active && this.refed;
      },
      refresh() {
        if (entry.cleared) return this;
        this.active = true;
        entry.due = __quenchHostNow() + entry.delay;
        entry.order = __quenchHostTimerOrder++;
        __quenchHostTimers.set(entry.id, entry);
        return this;
      },
      [Symbol.toPrimitive]() {
        return entry.id;
      }
    };
    Symbol.dispose ||= Symbol("dispose");
    handle[Symbol.dispose] = () => __quenchHostClear(handle);
    Object.defineProperty(handle, "__quenchHostEntry", { value: entry });
    entry.handle = handle;
    return handle;
  };
  const __quenchHostClear = (handle) => {
    const entry =
      __quenchHostTimers.get(Number(handle)) || handle?.__quenchHostEntry;
    if (!entry) return;
    entry.cleared = true;
    entry.handle.active = false;
    __quenchHostTimers.delete(entry.id);
  };
  const __quenchHostSchedule = (callback, delay, args, repeat) => {
    const entry = {
      id: __quenchHostTimerId++,
      callback,
      args,
      delay: Math.max(0, __nodeTimerDelay(delay)),
      due: __quenchHostNow() + Math.max(0, __nodeTimerDelay(delay)),
      repeat,
      cleared: false,
      order: __quenchHostTimerOrder++,
      domain: globalThis.__quench_active_domain,
      resource: globalThis.__nodeCurrentAsyncResource
    };
    __quenchHostTimers.set(entry.id, entry);
    const handle = __quenchHostHandle(entry);
    handle[Symbol.toPrimitive] = () => entry.id;
    return handle;
  };
  globalThis.setTimeout = (callback, delay = 0, ...args) => {
    if (typeof callback !== "function") {
      throw new TypeError('The "callback" argument must be of type function');
    }
    return __quenchHostSchedule(callback, delay, args, false);
  };
  globalThis.clearTimeout = __quenchHostClear;
  globalThis.setInterval = (callback, delay = 0, ...args) => {
    if (typeof callback !== "function") {
      throw new TypeError('The "callback" argument must be of type function');
    }
    return __quenchHostSchedule(callback, delay, args, true);
  };
  globalThis.clearInterval = __quenchHostClear;
  globalThis.setImmediate = (callback, ...args) => {
    if (typeof callback !== "function") {
      throw new TypeError('The "callback" argument must be of type function');
    }
    return __quenchHostSchedule(callback, 0, args, false);
  };
  globalThis.clearImmediate = __quenchHostClear;
  globalThis.__nodeTimers = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
    setImmediate: globalThis.setImmediate,
    clearImmediate: globalThis.clearImmediate
  };
  globalThis.__quench_timer_next_delay = () => {
    let next = -1;
    const now = __quenchHostNow();
    for (const entry of __quenchHostTimers.values()) {
      if (!entry.handle.refed) continue;
      const delay = Math.max(0, entry.due - now);
      if (next < 0 || delay < next) next = delay;
    }
    return next;
  };
  globalThis.__quench_timer_poll = () => {
    const now = __quenchHostNow();
    const hasRefedTimer = [...__quenchHostTimers.values()].some(
      (entry) => entry.handle.refed && entry.handle.active
    );
    const due = [...__quenchHostTimers.values()]
      .filter(
        (entry) =>
          entry.due <= now &&
          entry.handle.active &&
          (entry.handle.refed || hasRefedTimer)
      )
      .sort((a, b) => a.due - b.due || a.order - b.order);
    for (const entry of due) {
      if (!entry.handle.active) continue;
      if (entry.repeat) entry.due = now + entry.delay;
      else __quenchHostTimers.delete(entry.id);
      const previousResource = globalThis.__nodeCurrentAsyncResource;
      globalThis.__nodeCurrentAsyncResource = entry.resource;
      try {
        globalThis.__quench_work_generation =
          (globalThis.__quench_work_generation || 0) + 1;
        if (entry.domain) {
          entry.domain.run(() =>
            entry.callback.apply(entry.handle, entry.args)
          );
        } else entry.callback.apply(entry.handle, entry.args);
      } finally {
        globalThis.__nodeCurrentAsyncResource = previousResource;
      }
      if (!entry.repeat && !__quenchHostTimers.has(entry.id)) {
        entry.handle.active = false;
      }
    }
  };
}
"#;
