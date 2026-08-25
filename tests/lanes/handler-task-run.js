var KIND_WORK = 1;
var DATA_SIZE = 4;
var calls = 0;
var scheduler = {
  queue: function (packet) { calls++; return packet; },
  suspendCurrent: function () { calls++; return null; },
};
function Packet(kind) { this.link = null; this.kind = kind; this.a1 = 0; this.a2 = new Array(DATA_SIZE); }
Packet.prototype.addTo = function (queue) {
  this.link = null;
  if (queue == null) return this;
  var peek, next = queue;
  while ((peek = next.link) != null) next = peek;
  next.link = this;
  return queue;
};
function HandlerTask() { this.scheduler = scheduler; this.v1 = null; this.v2 = null; }
HandlerTask.prototype.run = function (packet) {
  if (packet != null) {
    if (packet.kind == KIND_WORK) this.v1 = packet.addTo(this.v1);
    else this.v2 = packet.addTo(this.v2);
  }
  if (this.v1 != null) {
    var count = this.v1.a1;
    var v;
    if (count < DATA_SIZE) {
      if (this.v2 != null) {
        v = this.v2; this.v2 = this.v2.link;
        v.a1 = this.v1.a2[count]; this.v1.a1 = count + 1;
        return this.scheduler.queue(v);
      }
    } else {
      v = this.v1; this.v1 = this.v1.link;
      return this.scheduler.queue(v);
    }
  }
  return this.scheduler.suspendCurrent();
};
var task = new HandlerTask();
var work = new Packet(KIND_WORK);
for (var j = 0; j < DATA_SIZE; j++) work.a2[j] = j + 3;
task.run(work);
var checksum = 0;
for (var i = 0; i < 20000; i++) {
  var device = new Packet(0);
  var result = task.run(device);
  checksum += result == null ? 0 : result.a1;
  if (task.v1 == null) { work = new Packet(KIND_WORK); for (var k = 0; k < DATA_SIZE; k++) work.a2[k] = k + 3; task.run(work); }
}
console.log(checksum + calls);
