const __quenchSpawnValidationRequire = globalThis.require;
const __quenchSpawnValidationChildProcess =
  __quenchSpawnValidationRequire("child_process");
const __quenchSpawnValidated = __quenchSpawnValidationChildProcess.spawn;
__quenchSpawnValidationChildProcess.spawn = (...args) => {
  const command = args[0];
  const options = args[1];
  const third = args[2];
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
  if (
    options !== undefined &&
    options !== null &&
    !Array.isArray(options) &&
    typeof options !== "object"
  ) {
    const error = new TypeError("The args argument must be an array");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    third !== undefined &&
    (third === null || typeof third !== "object" || Array.isArray(third))
  ) {
    const error = new TypeError("The options argument must be an object");
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const childOptions =
    third && typeof third === "object"
      ? third
      : options && typeof options === "object" && !Array.isArray(options)
        ? options
        : undefined;
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
  return __quenchSpawnValidated(...args);
};
