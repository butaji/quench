NodeBuffer.prototype.write = function write(
  value,
  offset = 0,
  length,
  encoding = "utf8",
) {
  if (typeof length === "string") {
    encoding = length;
    length = undefined;
  }
  const bytes = NodeBuffer.from(String(value), encoding);
  const available = this.length - offset;
  const count = Math.min(
    available,
    length === undefined ? available : Math.max(0, Number(length)),
    bytes.length,
  );
  this.set(bytes.subarray(0, count), offset);
  return count;
};
NodeBuffer.compare = (left, right) => {
  if (!(left instanceof Uint8Array)) {
    const error = new TypeError(
      `The "buf1" argument must be an instance of Buffer or Uint8Array.${
        __nodeBufferConcatReceived(left)
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  if (!(right instanceof Uint8Array)) {
    const error = new TypeError(
      `The "buf2" argument must be an instance of Buffer or Uint8Array.${
        __nodeBufferConcatReceived(right)
      }`,
    );
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  const a = NodeBuffer.from(left);
  const b = NodeBuffer.from(right);
  const length = Math.min(a.length, b.length);
  for (let i = 0; i < length; i++) {
    if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  }
  return a.length === b.length ? 0 : a.length < b.length ? -1 : 1;
};
const __nodeFinalBufferFrom = NodeBuffer.from;
NodeBuffer.from = (...args) => {
  const source = __nodeFinalBufferFrom.apply(NodeBuffer, args);
  if (source instanceof NodeBuffer) return source;
  const output = new NodeBuffer(source.length);
  output.set(source);
  return output;
};
