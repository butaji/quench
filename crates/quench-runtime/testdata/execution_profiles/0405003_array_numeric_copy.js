function copy(source, target) {
  for (var i = 0; i < source.length; i++) target[i] = source[i];
  return target[0] + target[target.length - 1];
}

copy([1, 2], [0, 0]);
var target = [0, 0, 0, 0];
function run() {
  return copy([19, 20, 21, 23], target);
}
function verify(result) {
if (result !== 42 || target.join(",") !== "19,20,21,23") {
  throw new Error("numeric copy mismatch");
}
  return result;
}
return { run: run, verify: verify };
