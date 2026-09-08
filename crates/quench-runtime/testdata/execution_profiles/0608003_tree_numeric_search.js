function find(root, key) {
  var node = root;
  while (node !== null) {
    if (key === node.key) return node.value;
    node = key < node.key ? node.left : node.right;
  }
  return -1;
}

var root = {
  key: 10,
  value: 1,
  left: { key: 5, value: 42, left: null, right: null },
  right: { key: 20, value: 7, left: null, right: null }
};
find(root, 20);
function run() {
  return find(root, 5);
}
function verify(result) {
if (result !== 42) {
  throw new Error("tree numeric search mismatch");
}
  return result;
}
return { run: run, verify: verify };
