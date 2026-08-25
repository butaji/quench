var calls = 0;
var STATE_SUSPENDED = 2;
function TaskControlBlock() { this.state = 0; }
TaskControlBlock.prototype.markAsSuspended = function () {
  this.state = this.state | STATE_SUSPENDED;
};
function Scheduler() { this.currentTcb = new TaskControlBlock(); }
Scheduler.prototype.suspendCurrent = function () {
  this.currentTcb.markAsSuspended();
  return this.currentTcb;
};
Scheduler.prototype.queue = function (packet) { calls += 2; return packet; };
Scheduler.prototype.holdCurrent = function () { calls += 4; return 4; };
var scheduler = new Scheduler();

function DeviceTask() {
  this.scheduler = scheduler;
  this.v1 = null;
}

DeviceTask.prototype.run = function (packet) {
  if (packet == null) {
    if (this.v1 == null) return this.scheduler.suspendCurrent();
    var v = this.v1;
    this.v1 = null;
    return this.scheduler.queue(v);
  } else {
    this.v1 = packet;
    return this.scheduler.holdCurrent();
  }
};

var task = new DeviceTask();
var checksum = 0;
for (var i = 0; i < 20000; i++) {
  checksum += task.run({ id: i });
  checksum += task.run(null).id;
  checksum += task.run(null).state;
}
console.log(checksum + calls);
