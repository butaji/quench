function sum(node) {
  var total = 0;
  while (node !== null) {
    total = total + node.value;
    node = node.next;
  }
  return total;
}

var tail = { value: 2, next: null };
var middle = { value: 10, next: tail };
var head = { value: 30, next: middle };
sum(head);
var result = sum(head);
if (result !== 42) {
  throw new Error("linked-list walk mismatch");
}
return result;
