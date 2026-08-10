const __quenchSpawnValidationRequire = globalThis.require;
const __quenchSpawnValidationChildProcess = __quenchSpawnValidationRequire(
  "child_process",
);
const __quenchSpawnValidated = __quenchSpawnValidationChildProcess.spawn;
const __quenchValidateSpawnCommand = (command) => {
  if (typeof command !== "string") {
    const error = new TypeError("The file argument must be of type string");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (command.length === 0) {
    const error = new TypeError("The file argument must not be empty");
    error.code = "ERR_INVALID_ARG_VALUE";
    throw error;
  }
};
const __quenchValidateSpawnArgs = (options) => {
  if (
    options !== undefined && options !== null &&
    !Array.isArray(options) && typeof options !== "object"
  ) {
    const error = new TypeError("The args argument must be an array");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
};
const __quenchValidateSpawnOptions = (third, options) => {
  if (
    third !== undefined &&
    (third === null || typeof third !== "object" || Array.isArray(third))
  ) {
    const error = new TypeError("The options argument must be an object");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  __quenchValidateSpawnIds(__quenchSpawnChildOptions(third, options));
};
const __quenchSpawnChildOptions = (third, options) =>
  third && typeof third === "object"
    ? third
    : options && typeof options === "object" && !Array.isArray(options)
    ? options
    : undefined;
const __quenchValidateSpawnIds = (childOptions) => {
  for (const field of ["uid", "gid"]) {
    if (
      childOptions?.[field] !== undefined &&
      (!Number.isSafeInteger(childOptions[field]) || childOptions[field] < 0)
    ) {
      const error = new RangeError(`The ${field} option is out of range`);
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
  }
};
__quenchSpawnValidationChildProcess.spawn = (...args) => {
  __quenchValidateSpawnCommand(args[0]);
  __quenchValidateSpawnArgs(args[1]);
  __quenchValidateSpawnOptions(args[2], args[1]);
  return __quenchSpawnValidated(...args);
};
