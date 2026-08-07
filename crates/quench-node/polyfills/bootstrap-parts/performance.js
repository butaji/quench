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
  timerify: (functionToWrap) => {
    if (typeof functionToWrap !== "function") {
      throw new TypeError('The "fn" argument must be a function');
    }
    const wrapped = (...args) => functionToWrap(...args);
    Object.defineProperty(wrapped, "name", {
      configurable: true,
      value: functionToWrap.name,
    });
    return wrapped;
  },
  toJSON: () => ({ timeOrigin: __nodeStartedAt }),
};
class NodePerformanceObserver {
  constructor(callback) {
    this.callback = callback;
  }
  observe() {}
  disconnect() {}
  takeRecords() {
    return [];
  }
}
globalThis.__nodePerfHooks = {
  performance: __nodePerformance,
  timerify: __nodePerformance.timerify,
  PerformanceObserver: NodePerformanceObserver,
};
const __nodePrototypeNames = new WeakMap();
const __nodeSetPrototypeOf = Object.setPrototypeOf;
Object.setPrototypeOf = (object, prototype) => {
  if (prototype === null && object && object.constructor?.name) {
    __nodePrototypeNames.set(object, object.constructor.name);
  }
  return __nodeSetPrototypeOf(object, prototype);
};
