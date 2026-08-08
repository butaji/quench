const __nodeSystemErrorNames = new Map([
  [-1, "EPERM"],
  [-2, "ENOENT"],
  [-13, "EACCES"],
  [-17, "EEXIST"],
  [-32, "EPIPE"],
  [-105, "ENOBUFS"]
]);
const __nodeUtilGetSystemErrorName = (errorNumber) => {
  if (typeof errorNumber !== "number") {
    const error = new TypeError(
      `The "err" argument must be of type number. Received type ${typeof errorNumber} (${String(
        errorNumber
      )})`
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!Number.isInteger(errorNumber) || errorNumber >= 0) {
    const error = new RangeError(
      `The value of "err" is out of range. It must be a negative integer. Received ${errorNumber}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return (
    __nodeSystemErrorNames.get(errorNumber) ||
    `Unknown system error ${errorNumber}`
  );
};
const __nodeUtilGetSystemErrorMessage = (errorNumber) =>
  __nodeUtilGetSystemErrorName(errorNumber);
const __nodeUtilExceptionWithHostPort = (
  errorNumber,
  syscall,
  address,
  port,
  additional
) => {
  const code = __nodeUtilGetSystemErrorName(errorNumber);
  const error = new Error(`${syscall} ${code}`);
  error.errno = errorNumber;
  error.code = code;
  error.syscall = syscall;
  error.address = address;
  if (port) {
    error.port = port;
    error.message += ` ${address}:${port}`;
  } else if (address) error.message += ` ${address}`;
  if (additional) error.message += ` - Local (${additional})`;
  return error;
};
const __nodeUtilErrnoException = (errorNumber, syscall) => {
  const error = new Error(
    `${syscall || ""} ${__nodeUtilGetSystemErrorName(errorNumber)}`.trim()
  );
  error.errno = errorNumber;
  error.code = __nodeUtilGetSystemErrorName(errorNumber);
  if (syscall) error.syscall = syscall;
  return error;
};
globalThis.__nodeUtil.getSystemErrorName = __nodeUtilGetSystemErrorName;
globalThis.__nodeUtil._exceptionWithHostPort = __nodeUtilExceptionWithHostPort;
globalThis.__nodeUtil._errnoException = __nodeUtilErrnoException;
globalThis.__nodeUtil.getSystemErrorMessage = __nodeUtilGetSystemErrorMessage;
globalThis.__nodeUtil.getSystemErrorMap = () =>
  new Map(
    [...__nodeSystemErrorNames].map(([number, name]) => [number, [name, name]])
  );
