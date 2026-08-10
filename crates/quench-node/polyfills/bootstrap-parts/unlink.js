const __quenchUnlinkRequire = globalThis.require;
const __quenchUnlinkFs = __quenchUnlinkRequire("fs");
const __quenchUnlinkPath = (value) =>
  typeof value === "string" ||
  value instanceof Uint8Array ||
  value instanceof globalThis.__nodeURL;
const __quenchUnlinkError = () => {
  const error = new TypeError(
    'The "path" argument must be of type string or an instance of Buffer or URL',
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  return error;
};
const __quenchUnlinkSync = __quenchUnlinkFs.unlinkSync;
__quenchUnlinkFs.unlinkSync = (value) => {
  if (!__quenchUnlinkPath(value)) throw __quenchUnlinkError();
  return __quenchUnlinkSync(value);
};
const __quenchUnlink = __quenchUnlinkFs.unlink;
__quenchUnlinkFs.unlink = (value, callback) => {
  if (!__quenchUnlinkPath(value)) throw __quenchUnlinkError();
  return __quenchUnlink(value, callback);
};

const __quenchUnlinkAsyncFs = globalThis.require("fs");
__quenchUnlinkAsyncFs.unlink = (value, callback) => {
  if (typeof callback !== "function") {
    throw new TypeError('The "callback" argument must be of type function');
  }
  if (!__quenchUnlinkPath(value)) throw __quenchUnlinkError();
  queueMicrotask(() => {
    try {
      __quenchUnlinkAsyncFs.unlinkSync(value);
      callback(null);
    } catch (error) {
      callback(error);
    }
  });
};
__quenchUnlinkAsyncFs.promises.unlink = (value) =>
  new Promise((resolve, reject) =>
    __quenchUnlinkAsyncFs.unlink(
      value,
      (error) => error ? reject(error) : resolve(),
    )
  );

const __quenchUnlinkPromiseFs = globalThis.require("fs");
__quenchUnlinkPromiseFs.promises.unlink = (value) =>
  new Promise((resolve, reject) =>
    __quenchUnlinkPromiseFs.unlink(
      value,
      (error) => error ? reject(error) : resolve(),
    )
  );
