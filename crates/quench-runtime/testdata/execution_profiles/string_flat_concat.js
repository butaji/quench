function join(prefix, value, suffix) {
  return prefix + value + suffix;
}

join("a", 1, "b");
var result = join("value=", 42, "!");
if (result !== "value=42!") {
  throw new Error("flat string concat mismatch");
}
return result;
