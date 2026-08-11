//! Polyfill: `copy-head`

pub const JS: &str = quench_js_check::checked_js!(r#"const __nodeBufferCopyNumber = (value) => {
  const result = Math.trunc(Number(value));
  return Number.isNaN(result) ? 0 : result;
};
const __nodeBufferCopyTarget = (target) =>
  target instanceof Uint8Array
    ? target
    : new Uint8Array(target.buffer, target.byteOffset, target.byteLength);
const __nodeBufferCopy = (
  source,
  target,
  targetStart,
  sourceStart,
  sourceEnd,
) => {
  if (targetStart >= target.length || sourceStart >= sourceEnd) return 0;
  const end = Math.min(sourceEnd, source.length);
  const count = Math.min(end - sourceStart, target.length - targetStart);
  const bytes = new Uint8Array(count);
  bytes.set(source.subarray(sourceStart, sourceStart + count));
  target.set(bytes, targetStart);
  return count;
};
const __nodeBufferCopyRangeError = (name, rule, value) => {
  const error = new RangeError(
    `The value of "${name}" is out of range. It must be ${rule}. Received ${value}`,
  );
  error.code = "ERR_OUT_OF_RANGE";
  return error;
};
const __nodeBufferCopyValidate = (source, target) => {
  if (!(source instanceof Uint8Array)) {
    throw Object.assign(new TypeError("Method Buffer.prototype.copy called on incompatible receiver"), { code: "ERR_INVALID_ARG_TYPE" });
  }
  if (!ArrayBuffer.isView(target)) {
    throw Object.assign(new TypeError('The "target" argument must be an instance of Uint8Array'), { code: "ERR_INVALID_ARG_TYPE" });
  }
};
const __nodeBufferConcatReceived = (value) => {
  if (value == null) return ` Received ${value}`;
  if (typeof value === "number") return ` Received type number (${value})`;
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (ArrayBuffer.isView(value)) return " Received an instance of Buffer";
  const name = value.constructor?.name === "NodeBuffer"
    ? "Buffer"
    : value.constructor?.name;
  return ` Received an instance of ${name || "Object"}`;
};
const __nodeBufferConcatValidate = (list, totalLength) => {
  if (!Array.isArray(list)) {
    const error = new TypeError(
      `The "list" argument must be an instance of Array.${
        __nodeBufferConcatReceived(
          list,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const invalidIndex = list.findIndex((item) => !ArrayBuffer.isView(item));
  if (invalidIndex >= 0) {
    const item = list[invalidIndex];
    const error = new TypeError(
      `The "list[${invalidIndex}]" argument must be an instance of Buffer or Uint8Array.${
        __nodeBufferConcatReceived(
          item,
        )
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (
    totalLength !== undefined &&
    (typeof totalLength !== "number" ||
      !Number.isInteger(totalLength) ||
      totalLength < 0)
  ) {
    const message = Number.isInteger(totalLength)
      ? `The value of "length" is out of range. It must be >= 0 && <= 9007199254740991. Received ${totalLength}`
      : `The value of "length" is out of range. It must be an integer. Received ${totalLength}`;
    throw Object.assign(new RangeError(message), { code: "ERR_OUT_OF_RANGE" });
  }
};
"#);
