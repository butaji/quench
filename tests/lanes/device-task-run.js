var calls = 0;
var scheduler = {
  suspendCurrent: function () { calls += 1; return 1; },
  queue: function (packet) { calls += 2; return packet; },
  holdCurrent: function () { calls += 4; return 4; },
};

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
  checksum += task.run(null);
}
console.log(checksum + calls);
