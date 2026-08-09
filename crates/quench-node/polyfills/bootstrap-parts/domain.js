const __quenchOriginalRequireWithDomain = globalThis.require;
const __quenchMarkDomainError = (error, domain, thrown) => {
  Object.defineProperty(error, "domain", {
    configurable: true,
    enumerable: false,
    value: domain,
    writable: true
  });
  error.domainThrown = thrown;
};
class __quenchDomain {
  constructor() {
    this.members = [];
    this._listeners = new Map();
    this._active = false;
    this.disposed = false;
  }
  on(event, listener) {
    const listeners = this._listeners.get(event) || [];
    listeners.push(listener);
    this._listeners.set(event, listeners);
    return this;
  }
  once(event, listener) {
    const wrapped = (...args) => {
      this.removeListener(event, wrapped);
      listener(...args);
    };
    return this.on(event, wrapped);
  }
  removeListener(event, listener) {
    this._listeners.set(
      event,
      (this._listeners.get(event) || []).filter((item) => item !== listener)
    );
    return this;
  }
  emit(event, ...args) {
    for (const listener of [...(this._listeners.get(event) || [])]) {
      listener(...args);
    }
    return (this._listeners.get(event) || []).length > 0;
  }
  listeners(event) {
    return [...(this._listeners.get(event) || [])];
  }
  enter() {
    if (!this.disposed) {
      const stack =
        globalThis.__quench_domain_stack ||
        (globalThis.__quench_domain_stack = []);
      stack.push(this);
      globalThis.__quench_active_domain = this;
      if (globalThis.process) globalThis.process.domain = this;
      this._active = true;
    }
    return this;
  }
  exit() {
    const stack = globalThis.__quench_domain_stack || [];
    const index = stack.lastIndexOf(this);
    if (index >= 0) stack.splice(index);
    globalThis.__quench_active_domain = stack.at(-1);
    if (globalThis.process) globalThis.process.domain = stack.at(-1);
    this._active = false;
    return this;
  }
  run(callback, ...args) {
    this.enter();
    try {
      return callback(...args);
    } catch (error) {
      if (!this.listeners("error").length) throw error;
      __quenchMarkDomainError(error, this, true);
      this.emit("error", error);
    } finally {
      this.exit();
    }
  }
  add(member) {
    if (!this.members.includes(member)) this.members.push(member);
    Object.defineProperty(member, "domain", {
      configurable: true,
      enumerable: false,
      value: this,
      writable: true
    });
    return this;
  }
  remove(member) {
    this.members = this.members.filter((item) => item !== member);
    if (member.domain === this) delete member.domain;
    return this;
  }
  bind(callback) {
    return (...args) =>
      this.run(() => {
        try {
          return callback(...args);
        } catch (error) {
          __quenchMarkDomainError(error, this, true);
          this.emit("error", error);
        }
      });
  }
  intercept(callback) {
    return (error, ...args) => {
      if (error instanceof Error) {
        __quenchMarkDomainError(error, this, false);
        error.domainBound = callback;
        this.emit("error", error);
        return;
      }
      return this.run(() => callback(...args));
    };
  }
  dispose() {
    this.members = [];
    this.disposed = true;
    this.exit();
  }
}
const __quenchDomainModule = {
  Domain: __quenchDomain,
  create: () => new __quenchDomain(),
  createDomain: () => new __quenchDomain(),
  _stack: []
};
Object.defineProperty(__quenchDomainModule, "active", {
  enumerable: true,
  get: () => globalThis.__quench_active_domain || null
});
Object.defineProperty(__quenchDomainModule, "_stack", {
  enumerable: true,
  get: () => globalThis.__quench_domain_stack || []
});
if (!globalThis.__quench_domain_promises_patched && globalThis.Promise) {
  const originalThen = Promise.prototype.then;
  const wrap = (callback, activeDomain) =>
    typeof callback !== "function"
      ? callback
      : (...args) =>
          activeDomain
            ? activeDomain.run(() => callback(...args))
            : callback(...args);
  Promise.prototype.then = function (onFulfilled, onRejected) {
    const activeDomain = globalThis.__quench_active_domain;
    return originalThen.call(
      this,
      wrap(onFulfilled, activeDomain),
      wrap(onRejected, activeDomain)
    );
  };
  globalThis.__quench_domain_promises_patched = true;
}
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "domain") {
    return __quenchDomainModule;
  }
  return __quenchOriginalRequireWithDomain(specifier);
};
