function join(prefix, value, suffix) {
  return prefix + value + suffix;
}

join("a", 1, "b");
function run() {
  return join("value=", 42, "!");
}
function verify(result) {
if (result !== "value=42!") {
  throw new Error("flat string concat mismatch");
}
  return result;
}
return { run: run, verify: verify };
