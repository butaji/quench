const __nodeBufferFromReceived = (value) => {
  if (value == null) return ` Received ${value}`;
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (typeof value === "symbol")
    return ` Received type symbol (${String(value)})`;
  if (typeof value === "bigint") return ` Received type bigint (${value}n)`;
  if (typeof value === "function") return " Received function ";
  const name = value.constructor?.name || "Object";
  return ` Received an instance of ${name}`;
};
