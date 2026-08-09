NodeBuffer.prototype.write = function write(
  value,
  offset = 0,
  length,
  encoding = "utf8"
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
    bytes.length
  );
  this.set(bytes.subarray(0, count), offset);
  return count;
};
