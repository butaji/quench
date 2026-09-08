"use strict";
function assert(condition, message) {
  if (!condition) throw new Error("state-dispatch assertion failed: " + message);
}
function Task(id, state) {
  this.id = id;
  this.state = state;
  this.value = 0;
}
Task.prototype.run = function () {
  this.value += this.id;
  this.state ^= 1;
  return this.value;
};
function schedule(tasks, rounds) {
  var total = 0;
  for (var round = 0; round < rounds; round++) {
    for (var i = 0; i < tasks.length; i++) {
      var task = tasks[i];
      if ((task.state & 2) === 0) total += task.run();
    }
  }
  return total;
}
var result = schedule([
  new Task(1, 0), new Task(2, 1), new Task(3, 2), new Task(4, 3)
], 4);
assert(result === 30, "exact scheduler state result");
return result;
