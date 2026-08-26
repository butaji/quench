var STATE_RUNNING = 0;
var STATE_RUNNABLE = 1;
var STATE_SUSPENDED = 2;
var STATE_HELD = 4;
var STATE_SUSPENDED_RUNNABLE = STATE_SUSPENDED | STATE_RUNNABLE;

function DeviceTask(scheduler) { this.scheduler = scheduler; this.v1 = null; }
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

function TaskControlBlock(link, id, priority, queue, task) {
  this.link = link;
  this.id = id;
  this.priority = priority;
  this.queue = queue;
  this.task = task;
  this.state = queue == null ? STATE_SUSPENDED : STATE_SUSPENDED_RUNNABLE;
}
TaskControlBlock.prototype.isHeldOrSuspended = function () {
  return (this.state & STATE_HELD) != 0 || this.state == STATE_SUSPENDED;
};
TaskControlBlock.prototype.markAsSuspended = function () {
  this.state = this.state | STATE_SUSPENDED;
};
TaskControlBlock.prototype.run = function () {
  var packet;
  if (this.state == STATE_SUSPENDED_RUNNABLE) {
    packet = this.queue;
    this.queue = packet.link;
    if (this.queue == null) this.state = STATE_RUNNING;
    else this.state = STATE_RUNNABLE;
  } else {
    packet = null;
  }
  return this.task.run(packet);
};

function Scheduler() {
  this.list = null;
  this.currentTcb = null;
  this.currentId = null;
}
Scheduler.prototype.suspendCurrent = function () {
  this.currentTcb.markAsSuspended();
  return this.currentTcb;
};
Scheduler.prototype.queue = function (packet) { return packet; };
Scheduler.prototype.holdCurrent = function () { return this.currentTcb; };
Scheduler.prototype.schedule = function () {
  this.currentTcb = this.list;
  while (this.currentTcb != null) {
    if (this.currentTcb.isHeldOrSuspended()) this.currentTcb = this.currentTcb.link;
    else {
      this.currentId = this.currentTcb.id;
      this.currentTcb = this.currentTcb.run();
    }
  }
};

var scheduler = new Scheduler();
var device = new DeviceTask(scheduler);
var tcb = new TaskControlBlock(null, 0, 1, null, device);
scheduler.list = tcb;
var checksum = 0;
for (var i = 0; i < 20000; i++) {
  tcb.state = STATE_RUNNING;
  scheduler.schedule();
  checksum += tcb.state;
}
console.log(checksum);
