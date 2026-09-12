//! Polyfill: `performance`

pub const JS: &str = quench_js_check::checked_js!(
    r#"const __nodeStartedAt = Date.now();
const __nodePerformanceEntries = [];
const __nodePerformanceMarks = new Map();
const __nodePerformanceObservers = new Set();
const __nodePerformance = {
  now: () => Date.now() - __nodeStartedAt,
  timeOrigin: __nodeStartedAt,
  mark: (name) => {
    const entry = {
      name: String(name),
      entryType: "mark",
      startTime: __nodePerformance.now(),
      duration: 0,
    };
    (
      __nodePerformanceMarks.get(entry.name) ||
      __nodePerformanceMarks.set(entry.name, []).get(entry.name)
    ).push(entry);
    __nodePerformanceEntries.push(entry);
    return entry;
  },
  measure: (name, startMark, endMark) => {
    const start = startMark
      ? __nodePerformanceMarks.get(String(startMark))?.at(-1)?.startTime || 0
      : 0;
    const end = endMark
      ? __nodePerformanceMarks.get(String(endMark))?.at(-1)?.startTime ||
        __nodePerformance.now()
      : __nodePerformance.now();
    const entry = {
      name: String(name),
      entryType: "measure",
      startTime: start,
      duration: Math.max(0, end - start),
    };
    __nodePerformanceEntries.push(entry);
    return entry;
  },
  clearMarks: (name) => {
    if (name === undefined) __nodePerformanceMarks.clear();
    else __nodePerformanceMarks.delete(String(name));
    for (let index = __nodePerformanceEntries.length - 1; index >= 0; index--) {
      if (
        __nodePerformanceEntries[index].entryType === "mark" &&
        (name === undefined ||
          __nodePerformanceEntries[index].name === String(name))
      ) {
        __nodePerformanceEntries.splice(index, 1);
      }
    }
  },
  clearMeasures: (name) => {
    for (let index = __nodePerformanceEntries.length - 1; index >= 0; index--) {
      if (
        __nodePerformanceEntries[index].entryType === "measure" &&
        (name === undefined ||
          __nodePerformanceEntries[index].name === String(name))
      ) {
        __nodePerformanceEntries.splice(index, 1);
      }
    }
  },
  getEntries: () => __nodePerformanceEntries.slice(),
  getEntriesByName: (name, entryType) =>
    __nodePerformanceEntries.filter(
      (entry) =>
        entry.name === String(name) &&
        (entryType === undefined || entry.entryType === String(entryType)),
    ),
  getEntriesByType: (entryType) =>
    __nodePerformanceEntries.filter(
      (entry) => entry.entryType === String(entryType),
    ),
  timerify: (functionToWrap, options) => {
    if (typeof functionToWrap !== "function") {
      const error = new TypeError('The "fn" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    if (options?.histogram !== undefined &&
        (typeof options.histogram !== "object" || options.histogram === null ||
         typeof options.histogram.record !== "function")) {
      const error = new TypeError('The "options.histogram" argument must be an instance of RecordableHistogram');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    const wrapped = function (...args) {
      const startTime = __nodePerformance.now();
      let result;
      if (new.target) {
        // Direct construction must retain the wrapped constructor's
        // prototype (Node's timerify wrapper is transparent to `instanceof`).
        // A derived constructor still supplies its own newTarget.
        const newTarget = new.target === wrapped ? functionToWrap : new.target;
        result = Reflect.construct(functionToWrap, args, newTarget);
      } else {
        result = Reflect.apply(functionToWrap, this, args);
      }
      const entry = {
        name: functionToWrap.name,
        entryType: "function",
        startTime,
        duration: Math.max(0, __nodePerformance.now() - startTime),
      };
      if (options?.histogram) {
        options.histogram.record(Math.max(1, entry.duration));
      }
      args.forEach((value, index) => { entry[index] = value; });
      __nodePerformanceEntries.push(entry);
      for (const observer of __nodePerformanceObservers) {
        if (observer.entryTypes.includes("function")) {
          queueMicrotask(() => observer.callback({ getEntries: () => [entry] }));
        }
      }
      return result;
    };
    Object.defineProperty(wrapped, "name", {
      configurable: true,
      value: `timerified ${functionToWrap.name}`,
    });
    Object.defineProperty(wrapped, "length", {
      configurable: true,
      value: functionToWrap.length,
    });
    return wrapped;
  },
  // Node exposes the startup timing record alongside timeOrigin.  Keep the
  // record stable so callers can inspect it without a host round-trip.
  toJSON: () => ({
    nodeTiming: {
      name: "node",
      entryType: "node",
      startTime: 0,
      duration: __nodePerformance.now(),
      nodeStart: 0,
      v8Start: 0,
      bootstrapComplete: 0,
      environment: 0,
      loopStart: -1,
      loopExit: -1,
      idleTime: 0,
    },
    timeOrigin: __nodeStartedAt,
  }),
};
// Host modules report I/O observations through this narrow bridge.  The
// observer registry and entry queue stay centralized here, so native Rust
// implementations do not need to recreate PerformanceObserver semantics.
const __nodePerformanceRecord = (entryType, detail = {}, name = entryType) => {
  const entry = {
    name: String(name),
    entryType: String(entryType),
    startTime: __nodePerformance.now(),
    duration: 0,
    detail,
  };
  __nodePerformanceEntries.push(entry);
  for (const observer of __nodePerformanceObservers) {
    if (observer.entryTypes.includes(entry.entryType)) {
      queueMicrotask(() => observer.callback({ getEntries: () => [entry] }));
    }
  }
  return entry;
};
class NodePerformanceObserver {
  constructor(callback) {
    this.callback = callback;
    this.entryTypes = [];
  }
  observe(options = {}) {
    this.entryTypes = options.entryTypes || (options.type ? [options.type] : []);
    __nodePerformanceObservers.add(this);
  }
  disconnect() {
    __nodePerformanceObservers.delete(this);
  }
  takeRecords() {
    return [];
  }
}
const __nodePerfHooks = {
  performance: __nodePerformance,
  timerify: __nodePerformance.timerify,
  PerformanceObserver: NodePerformanceObserver,
  createHistogram: () => ({
    max: 0,
    record(value) {
      this.max = Math.max(this.max, Number(value));
    },
  }),
};
Object.defineProperty(globalThis, "__nodePerfHooks", {
  configurable: true,
  enumerable: false,
  value: __nodePerfHooks,
});
Object.defineProperty(globalThis, "__nodePerformanceRecord", {
  configurable: true,
  enumerable: false,
  value: __nodePerformanceRecord,
});
Object.defineProperty(globalThis, "performance", {
  configurable: true,
  enumerable: false,
  value: __nodePerformance,
});
const __nodePrototypeNames = new WeakMap();
const __nodeSetPrototypeOf = Object.setPrototypeOf;
Object.setPrototypeOf = (object, prototype) => {
  if (prototype === null && object && object.constructor?.name) {
    __nodePrototypeNames.set(object, object.constructor.name);
  }
  return __nodeSetPrototypeOf(object, prototype);
};
"#
);
