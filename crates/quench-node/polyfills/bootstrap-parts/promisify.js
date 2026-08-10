const __nodeUtilPromisifyCustom = Symbol.for("nodejs.util.promisify.custom");
const __nodeUtilPromisifyTypeError = (message) => {
  throw Object.assign(new TypeError(message), { code: "ERR_INVALID_ARG_TYPE" });
};
const __nodeUtilPromisify = (fn) => {
  if (typeof fn !== "function") {
    __nodeUtilPromisifyTypeError(
      'The "original" argument must be of type function.' +
        (globalThis.__nodeCommon?.invalidArgTypeHelper?.(fn) || ""),
    );
  }
  const custom = fn[__nodeUtilPromisifyCustom];
  if (custom !== undefined) {
    if (typeof custom !== "function") {
      __nodeUtilPromisifyTypeError(
        'The "util.promisify.custom" property must be of type function',
      );
    }
    custom[__nodeUtilPromisifyCustom] = custom;
    return custom;
  }
  const promisified = (...args) =>
    new Promise((resolve, reject) =>
      fn(
        ...args,
        (error, ...values) =>
          error
            ? reject(error)
            : resolve(values.length > 1 ? values : values[0]),
      )
    );
  Object.setPrototypeOf(promisified, Object.getPrototypeOf(fn));
  promisified[__nodeUtilPromisifyCustom] = promisified;
  return promisified;
};
globalThis.__nodeUtil.promisify = __nodeUtilPromisify;
globalThis.__nodeUtil.promisify.custom = __nodeUtilPromisifyCustom;
