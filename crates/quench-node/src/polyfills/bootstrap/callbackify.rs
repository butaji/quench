//! Polyfill: `callbackify`

pub const JS: &str = r#"const __quenchOriginalRequireWithCallbackify = globalThis.require;
const __quenchCallbackify = (fn) => {
  if (typeof fn !== "function") {
    throw new TypeError("The first argument must be a function");
  }
  return (...args) => {
    const callback = args.pop();
    if (typeof callback !== "function") {
      throw new TypeError("The last argument must be a function");
    }
    queueMicrotask(() => {
      try {
        const result = fn(...args);
        if (result && typeof result.then === "function") {
          result.then(
            (value) => callback(null, value),
            (error) => callback(error),
          );
        } else callback(null, result);
      } catch (error) {
        callback(error);
      }
    });
  };
};
globalThis.require = (specifier) =>
  String(specifier).replace(/^node:/, "") === "util"
    ? Object.assign({}, __quenchOriginalRequireWithCallbackify(specifier), {
      callbackify: __quenchCallbackify,
    })
    : __quenchOriginalRequireWithCallbackify(specifier);
"#;
