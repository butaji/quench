const __quenchSourceMapsProcess = globalThis.process;
const __quenchSetSourceMapsEnabled = (value) => {
  if (typeof value !== "boolean") {
    const error = new TypeError(
      'The "val" argument must be of type boolean [ERR_INVALID_ARG_TYPE]',
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  __quenchSourceMapsProcess.__sourceMapsEnabled = value;
};
Object.defineProperty(__quenchSourceMapsProcess, "setSourceMapsEnabled", {
  get: () => __quenchSetSourceMapsEnabled,
  set: () => {},
  configurable: true,
});
