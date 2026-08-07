const __nodeBufferFromReceived = (value) => {
  if (value == null) return ` Received ${value}`;
  if (typeof value === "string") return ` Received type string ('${value}')`;
  if (["number", "boolean"].includes(typeof value)) {
    return ` Received type ${typeof value} (${value})`;
  }
  if (typeof value === "symbol") {
    return ` Received type symbol (${String(value)})`;
  }
  if (typeof value === "bigint") return ` Received type bigint (${value}n)`;
  if (typeof value === "function") return ` Received function ${value.name}`;
  if (Object.getPrototypeOf(value) === null) {
    return " Received [Object: null prototype] {}";
  }
  const name = value.constructor?.name || "Object";
  return ` Received an instance of ${name}`;
};
