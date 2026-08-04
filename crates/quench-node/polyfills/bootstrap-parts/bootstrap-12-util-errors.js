const __nodeSystemErrorNames = new Map([
  [-1, "EPERM"],
  [-2, "ENOENT"],
  [-13, "EACCES"]
]);
const __nodeUtilGetSystemErrorName = (errorNumber) => {
  if (!Number.isInteger(errorNumber) || errorNumber >= 0) {
    const error = new RangeError(
      `The value of "err" is out of range. It must be a negative integer. Received ${errorNumber}`
    );
    error.code = "ERR_OUT_OF_RANGE";
    throw error;
  }
  return __nodeSystemErrorNames.get(errorNumber) || `UNKNOWN_${-errorNumber}`;
};
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
globalThis.__nodeUtil.getSystemErrorName = __nodeUtilGetSystemErrorName;
globalThis.__nodeUtil._exceptionWithHostPort = __nodeUtilExceptionWithHostPort;
