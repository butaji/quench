var STATE_RUNNABLE = 1;

function Packet(link, id) {
  this.link = link;
  this.id = id;
}

Packet.prototype.addTo = function (queue) {
  this.link = null;
  if (queue == null) return this;
  var peek, next = queue;
  while ((peek = next.link) != null) next = peek;
  next.link = this;
  return queue;
};

function TaskControlBlock(priority, queue) {
  this.priority = priority;
  this.queue = queue;
  this.state = 0;
}

TaskControlBlock.prototype.markAsRunnable = function () {
  this.state = this.state | STATE_RUNNABLE;
};

TaskControlBlock.prototype.checkPriorityAdd = function (task, packet) {
  if (this.queue == null) {
    this.queue = packet;
    this.markAsRunnable();
    if (this.priority > task.priority) return this;
  } else {
    this.queue = packet.addTo(this.queue);
  }
  return task;
};

function Scheduler(blocks, current) {
  this.blocks = blocks;
  this.currentTcb = current;
  this.currentId = 0;
  this.queueCount = 0;
}

Scheduler.prototype.queue = function (packet) {
  var t = this.blocks[packet.id];
  if (t == null) return t;
  this.queueCount++;
  packet.link = null;
  packet.id = this.currentId;
  return t.checkPriorityAdd(this.currentTcb, packet);
};

var current = new TaskControlBlock(1, null);
var head = new Packet(null, 0);
var target = new TaskControlBlock(2, null);
var scheduler = new Scheduler([target], current);
var packet = new Packet(null, 0);
var checksum = 0;
for (var i = 0; i < 20000; i++) {
  packet.id = 0;
  head.link = null;
  target.queue = i & 1 ? head : null;
  checksum += scheduler.queue(packet) === target ? 1 : 2;
}
console.log(checksum + scheduler.queueCount + target.state + (head.link === packet ? 1 : 0));
