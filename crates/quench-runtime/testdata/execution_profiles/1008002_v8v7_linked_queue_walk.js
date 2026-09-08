"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("linked-queue assertion failed: " + message);
}
function Packet(value) {
  this.value = value;
  this.link = null;
}
function append(head, packet) {
  if (head === null) return packet;
  var tail = head;
  while (tail.link !== null) tail = tail.link;
  tail.link = packet;
  return head;
}
function run() {
  var head = null;
  for (var i = 1; i <= 6; i++) head = append(head, new Packet(i));
  var result = 0;
  while (head !== null) {
    result += head.value;
    head = head.link;
  }
  return result;
}
function verify(result) {
assert(result === 21, "queue preserves insertion order");
  return result;
}
return { run: run, verify: verify };
