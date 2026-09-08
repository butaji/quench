function add(left, right, target) {
  for (var i = 0; i < left.length; i++) target[i] = left[i] + right[i];
  return target[0] + target[target.length - 1];
}

add([1], [2], [0]);
var target = [0, 0, 0];
function run() {
  return add([10, 20, 30], [9, 2, 12], target);
}
function verify(result) {
if (result !== 61 || target.join(",") !== "19,22,42") {
  throw new Error("elementwise add mismatch");
}
  return result;
}
return { run: run, verify: verify };
