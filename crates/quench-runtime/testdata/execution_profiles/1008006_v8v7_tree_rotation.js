"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("tree-rotation assertion failed: " + message);
}
function Node(key, left, right) {
  this.key = key;
  this.left = left;
  this.right = right;
}
function rotateRight(root) {
  var next = root.left;
  root.left = next.right;
  next.right = root;
  return next;
}
var root;
function run() {
  root = new Node(3, new Node(2, new Node(1, null, null), null), null);
  root = rotateRight(root);
  return root.key * 100 + root.left.key * 10 + root.right.key;
}
function verify(result) {
assert(result === 213, "rotation preserves links");
  return result;
}
return { run: run, verify: verify };
