const __quenchOriginalRequireWithDomain = globalThis.require;
class __quenchDomain {
  constructor() {
    this.members = [];
    this._active = false;
    this.disposed = false;
  }
  enter() {
    if (!this.disposed) this._active = true;
    return this;
  }
  exit() {
    this._active = false;
    return this;
  }
  run(callback) {
    this.enter();
    try {
      return callback();
    } finally {
      this.exit();
    }
  }
  add(member) {
    if (!this.members.includes(member)) this.members.push(member);
    return this;
  }
  remove(member) {
    this.members = this.members.filter((item) => item !== member);
    return this;
  }
  bind(callback) {
    return (...args) => this.run(() => callback(...args));
  }
  intercept(callback) {
    return this.bind(callback);
  }
  dispose() {
    this.members = [];
    this.disposed = true;
    this.exit();
  }
}
const __quenchDomainModule = {
  Domain: __quenchDomain,
  create: () => new __quenchDomain()
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "domain")
    return __quenchDomainModule;
  return __quenchOriginalRequireWithDomain(specifier);
};
