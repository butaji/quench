//! Polyfill: `child-process-validation`

pub const JS: &str = r#"const __quenchSpawnValidationRequire = globalThis.require;
const __quenchSpawnValidationChildProcess = __quenchSpawnValidationRequire(
  "child_process",
);
const __quenchSpawnValidated = __quenchSpawnValidationChildProcess.spawn;
const __quenchValidateSpawnCommand = (command) => {
  if (typeof command !== "string") {
    throw Object.assign(new TypeError("The file argument must be of type string"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (command.length === 0) {
    throw Object.assign(new TypeError("The file argument must not be empty"), { code: "ERR_INVALID_ARG_VALUE" });
  }
};
const __quenchValidateSpawnArgs = (options) => {
  if (
    options !== undefined && options !== null &&
    !Array.isArray(options) && typeof options !== "object"
  ) {
    throw Object.assign(new TypeError("The args argument must be an array"), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __quenchValidateSpawnOptions = (third, options) => {
  if (
    third !== undefined &&
    (third === null || typeof third !== "object" || Array.isArray(third))
  ) {
    throw Object.assign(new TypeError("The options argument must be an object"), { code: "ERR_INVALID_ARG_TYPE" });
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
      throw Object.assign(new RangeError(`The ${field} option is out of range`), { code: "ERR_OUT_OF_RANGE" });
    }
  }
};
__quenchSpawnValidationChildProcess.spawn = (...args) => {
  __quenchValidateSpawnCommand(args[0]);
  __quenchValidateSpawnArgs(args[1]);
  __quenchValidateSpawnOptions(args[2], args[1]);
  return __quenchSpawnValidated(...args);
};
"#;
