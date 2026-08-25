var STATE_HELD = 4;
var tcb = {
  state: 0,
  link: { value: 7 },
  markAsHeld: function () {
    this.state = this.state | STATE_HELD;
  },
};
var scheduler = {
  holdCount: 0,
  currentTcb: tcb,
  holdCurrent: function () {
    this.holdCount++;
    this.currentTcb.markAsHeld();
    return this.currentTcb.link;
  },
};
var result;
for (var i = 0; i < 20000; i++) result = scheduler.holdCurrent();
console.log(scheduler.holdCount + tcb.state + result.value);
