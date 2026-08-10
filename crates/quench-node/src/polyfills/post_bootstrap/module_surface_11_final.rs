//! Polyfill: `module-surface-11-final`

pub const JS: &str = r#"const __nodeURLSearchTag = Object.getOwnPropertyDescriptor(
  globalThis.__nodeURLSearchParams.prototype,
  Symbol.toStringTag,
);
if (!__nodeURLSearchTag || __nodeURLSearchTag.configurable) {
  Object.defineProperty(
    globalThis.__nodeURLSearchParams.prototype,
    Symbol.toStringTag,
    {
      configurable: true,
      enumerable: false,
      value: "URLSearchParams",
    },
  );
}
Object.defineProperty(globalThis.__nodeURLSearchParams.prototype, "size", {
  configurable: true,
  enumerable: true,
  get: Object.getOwnPropertyDescriptor(
    {
      get size() {
        return this._pairs.length;
      },
    },
    "size",
  ).get,
});
for (const name of ["append", "set", "delete", "sort"]) {
  const original = globalThis.__nodeURLSearchParams.prototype[name];
  globalThis.__nodeURLSearchParams.prototype[name] =
    Object.getOwnPropertyDescriptor(
      {
        [name](...args) {
          const result = original.apply(this, args);
          if (this.__nodeURLOwner) {
            this.__nodeURLOwner._search = this.toString() ? `?${this}` : "";
          }
          return result;
        },
      },
      name,
    ).value;
}
"#;
