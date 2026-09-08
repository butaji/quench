"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("tree-walk assertion failed: " + message);
}
function Node(key, right) {
  this.key = key;
  this.right = right;
}
var tree = new Node(1, new Node(2, new Node(3, new Node(4, null))));
var result = 0;
var current = tree;
while (current.right !== null) {
  result += current.key;
  current = current.right;
}
result += current.key;
assert(result === 10, "right-spine traversal");
return result;
