var ID_HANDLER_A = 1;
var ID_HANDLER_B = 2;
var DATA_SIZE = 4;
var calls = 0;
var scheduler = {
  suspendCurrent: function () { calls++; return 0; },
  queue: function (packet) { calls++; return packet.id; },
};

function WorkerTask(v1, v2) {
  this.scheduler = scheduler;
  this.v1 = v1;
  this.v2 = v2;
}

WorkerTask.prototype.run = function (packet) {
  if (packet == null) return this.scheduler.suspendCurrent();
  else {
    if (this.v1 == ID_HANDLER_A) this.v1 = ID_HANDLER_B;
    else this.v1 = ID_HANDLER_A;
    packet.id = this.v1;
    packet.a1 = 0;
    for (var i = 0; i < DATA_SIZE; i++) {
      this.v2++;
      if (this.v2 > 26) this.v2 = 1;
      packet.a2[i] = this.v2;
    }
    return this.scheduler.queue(packet);
  }
};

var task = new WorkerTask(ID_HANDLER_A, 1);
var packet = { id: 0, a1: 1, a2: new Array(DATA_SIZE) };
var checksum = task.run(null);
for (var i = 0; i < 20000; i++) checksum += task.run(packet);
console.log(checksum + calls + packet.a2[0] + packet.a2[3] + task.v2);
